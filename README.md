# Skate

Sort of Kubernetes...

Kubernetes manifest compatible.
Will support only a subset of resources and only a subset of their functionality:

- Deployments
- Pods
- DaemonSets
- Service: ExternalName only (w.i.p)
- Ingress (w.i.p)

Currently uses vendored ssh, plan is to move to openssh and use the native binary on the host.


## Registering nodes

```shell
skate create node --name foo --host bar
```

This will ensure all hosts are provisioned with `skatelet`, the agent

## Playing with objects
```shell
skate get pods

skate get nodes

skate get deployments
```

## Refreshing state (usually done automatically)
```shell
skate refresh
```

## Deploying manifests

```shell
skate apply -f manifest.yaml
```

## Developing

On mac I've been using cross for cross compilation:

```shell
make armv7
make armv6
```
