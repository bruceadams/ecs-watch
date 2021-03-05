# ECS Watch

A command line tool that watches for changes in the tasks of
an AWS Elastic Container Service cluster.

```bash
$ ecs-watch --help
ecs-watch
Watch AWS Elastic Container Service (ECS) cluster changes

USAGE:
    ecs-watch [FLAGS] [OPTIONS] --aws-profile <aws-profile> --cluster <cluster>

FLAGS:
    -d, --detail      Output the full task description response
    -h, --help        Prints help information
    -o, --one-shot    Output the summary once and exit. The default is to
                      continue to run, printing a new summary when anything in
                      the summary changes
    -V, --version     Prints version information

OPTIONS:
    -p, --aws-profile <aws-profile>
            AWS source profile to use. This name references an entry in
            ~/.aws/credentials [env: AWS_PROFILE=]

    -r, --aws-region <aws-region>
            AWS region to target [env: AWS_DEFAULT_REGION=] [default: us-east-1]

    -c, --cluster <cluster>
            Cluster name to watch [env: AWS_ECS_CLUSTER=]
```
