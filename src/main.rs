use ansi_term::Style;
use chrono::{NaiveDateTime, Utc};
use clap::{AppSettings::ColoredHelp, Clap};
use rusoto_core::{region::Region, RusotoError};
use rusoto_ecs::{
    Container, DescribeTasksError, DescribeTasksRequest, Ecs, EcsClient, ListTasksError,
    ListTasksRequest,
};
use snafu::{ResultExt, Snafu};
use std::{default::Default, env, str::FromStr};
use tokio::time::delay_for;

/// Watch AWS Elastic Container Service (ECS) cluster changes
#[derive(Clap, Clone, Debug)]
#[clap(global_setting = ColoredHelp)]
pub struct Args {
    /// AWS source profile to use. This name references an entry in ~/.aws/credentials
    #[clap(env = "AWS_PROFILE", long, short = "p")]
    aws_profile: String,
    /// AWS region to target.
    #[clap(
        default_value = "us-east-1",
        env = "AWS_DEFAULT_REGION",
        long,
        short = "r"
    )]
    aws_region: String,
    /// Cluster name to watch.
    #[clap(env = "AWS_ECS_CLUSTER", long, short)]
    cluster: String,
    /// Output the full task description response
    #[clap(long, short)]
    detail: bool,
    /// Output the summary once and exit. The default is to continue to run,
    /// printing a new summary when anything in the summary changes.
    #[clap(long, short)]
    one_shot: bool,
}

#[derive(Debug, Snafu)]
enum Error {
    #[snafu(display("Failed to lookup tasks for cluster \"{}\": {}", cluster_name, source))]
    TaskListLookup {
        cluster_name: String,
        source: RusotoError<ListTasksError>,
    },
    #[snafu(display(
        "Failed to lookup task definitions for cluster \"{}\": {}",
        cluster_name,
        source
    ))]
    TaskDescribe {
        cluster_name: String,
        source: RusotoError<DescribeTasksError>,
    },
    #[snafu(display("Failed to find ECS cluster \"{}\"", cluster_name))]
    ClusterNotFound { cluster_name: String },
}

#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub struct TaskSummary {
    date_time: NaiveDateTime,
    // desired_status: String,
    last_status: String,
    task_version: String,
    images: Vec<String>,
}

async fn tasks(ecs_client: &EcsClient, cluster_name: &str) -> Result<Vec<String>, Error> {
    let list_tasks_request = ListTasksRequest {
        cluster: Some(cluster_name.to_owned()),
        ..Default::default()
    };

    let response = ecs_client
        .list_tasks(list_tasks_request)
        .await
        .context(TaskListLookup { cluster_name })?;

    match response.task_arns {
        Some(tasks) => Ok(tasks),
        None => Err(Error::ClusterNotFound {
            cluster_name: cluster_name.to_owned(),
        }),
    }
}

const DATE_TIME_FORMAT: &str = "%F %T";

fn print_summary(summary: &[TaskSummary]) {
    println!("{}", Utc::now().format(DATE_TIME_FORMAT));
    for (index, task) in summary.iter().enumerate() {
        let line = format!(
            "{}  {:14} {} {:?}",
            task.date_time.format(DATE_TIME_FORMAT),
            task.last_status,
            task.task_version,
            task.images
        );
        // Look ahead: if the next timestamp is far past the current one,
        // underline the current to highlight the time break.
        if index + 1 < summary.len()
            && summary[index + 1].date_time - task.date_time >= chrono::Duration::hours(1)
        {
            println!("{}", Style::new().underline().paint(line))
        } else {
            println!("{}", line)
        }
    }
}

/// How long to sleep to get the next whole number of seconds.
fn sleep_duration(seconds: u64) -> std::time::Duration {
    let now = Utc::now();
    // let now_seconds = now.timestamp();
    let now_millis = now.timestamp_subsec_millis() as u64;
    std::time::Duration::from_millis(1000 * seconds - now_millis)
}

async fn watch(ecs_client: &EcsClient, cluster_name: &str) -> Result<(), Error> {
    let mut old_summary = task_summary(&ecs_client, cluster_name).await?;
    print_summary(&old_summary);

    loop {
        delay_for(sleep_duration(2)).await;

        let new_summary = task_summary(&ecs_client, cluster_name).await?;
        if old_summary != new_summary {
            print_summary(&new_summary);
            old_summary = new_summary;
        }
    }
}

/// Timestamps from AWS are floating point seconds to millisecond precision.
fn naive_date_time(timestamp: &f64) -> NaiveDateTime {
    let seconds = timestamp.to_owned() as i64;
    let milliseconds = (1000.0 * (timestamp - seconds as f64)).round() as u32;
    NaiveDateTime::from_timestamp(seconds, 1_000_000 * milliseconds)
}

fn newest_time(times: &[Option<f64>]) -> NaiveDateTime {
    let mut fs: Vec<f64> = times
        .iter()
        .filter(|time| time.is_some())
        .map(|time| time.unwrap())
        .collect();
    fs.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let now = Utc::now().timestamp() as f64;
    naive_date_time(fs.last().unwrap_or(&now))
}

/// Return a short task definition
fn task_version(task_definition_arn: &Option<String>) -> String {
    task_definition_arn
        .clone()
        .unwrap_or_default()
        .split('/')
        .last()
        .unwrap_or_default()
        .to_owned()
}

/// Return a short image name, dropping the first part.
fn short_image(image: &Option<String>) -> String {
    match image {
        Some(image) => {
            if image.split('/').count() > 1 {
                image.split('/').skip(1).collect::<Vec<&str>>().join("/")
            } else {
                image.to_owned()
            }
        }
        None => String::new(),
    }
}

/// Return a Vec of container images
fn images(containers: &Option<Vec<Container>>) -> Vec<String> {
    containers
        .clone()
        .unwrap_or_default()
        .iter()
        .map(|container| short_image(&container.image))
        .collect()
}

async fn task_summary(
    ecs_client: &EcsClient,
    cluster_name: &str,
) -> Result<Vec<TaskSummary>, Error> {
    let describe_tasks_request = DescribeTasksRequest {
        cluster: Some(cluster_name.to_owned()),
        tasks: tasks(ecs_client, cluster_name).await?,
        ..Default::default()
    };

    let result = ecs_client
        .describe_tasks(describe_tasks_request)
        .await
        .context(TaskDescribe { cluster_name })?;
    let mut task_list: Vec<TaskSummary> = result
        .tasks
        .unwrap_or_default()
        .iter()
        .map(|task| TaskSummary {
            date_time: newest_time(&[
                task.connectivity_at,
                task.created_at,
                task.execution_stopped_at,
                task.pull_started_at,
                task.pull_stopped_at,
                task.started_at,
            ]),
            task_version: task_version(&task.task_definition_arn),
            last_status: task.last_status.clone().unwrap_or_default(),
            images: images(&task.containers),
        })
        .collect();
    task_list.sort_by(|a, b| a.date_time.partial_cmp(&b.date_time).unwrap());
    Ok(task_list)
}

async fn detailed(ecs_client: &EcsClient, cluster_name: &str) -> Result<(), Error> {
    let describe_tasks_request = DescribeTasksRequest {
        cluster: Some(cluster_name.to_owned()),
        tasks: tasks(ecs_client, cluster_name).await?,
        ..Default::default()
    };

    let result = ecs_client
        .describe_tasks(describe_tasks_request)
        .await
        .context(TaskDescribe { cluster_name })?;
    println!("{:#?}", result);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), exitfailure::ExitFailure> {
    let args = Args::parse();

    env::set_var("AWS_PROFILE", &args.aws_profile);
    let region = Region::from_str(&args.aws_region)?;
    let ecs_client = EcsClient::new(region.clone());
    if args.detail {
        detailed(&ecs_client, &args.cluster).await?
    };
    if args.one_shot {
        let summary = task_summary(&ecs_client, &args.cluster).await?;
        print_summary(&summary);
    } else {
        watch(&ecs_client, &args.cluster).await?;
    }
    Ok(())
}
