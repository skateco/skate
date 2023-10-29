# Skate

Sort of Kubernetes...

Kubernetes manifest compatible.
Will support only a subset of resources and only a subset of their functionality:

- Deployments
- Pods
- Service: ExternalName only
- Ingress

## Architecture

```puml
actor Human
package local {
[Human] -> [Skate]
}
package "remote host" {
[Skate] <-> [Skatelet]: ssh
}
```

## Registering nodes

A `.hosts.yaml` file is required, with a list of hosts to manage.
```yaml
---
hosts:
  - host: host-a
  - host: host-b
```
Then ensure all hosts are provisioned with `skatelet`, the agent
```shell
skate up
```

## Deploying manifests
```shell
skate apply -f manifest.yaml
```


## Developing

On mac I've been using cross for cross compilation:
```shell
cargo install cross --git https://github.com/cross-rs/cross
```
Then 

```shell
# ras-pi 1
cross build --bin skatelet --target arm-unknown-linux-gnueabi
# ras-pi 2
cross build --bin skatelet --target armv7-unknown-linux-gnueabi
```