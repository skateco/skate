# Skate

Sort of Kubernetes...

An extremely low footprint mini paas for scheduling resources on a small number of hosts.

**Kubernetes manifest compatible.**
Will support only a subset of resources and only a subset of their functionality:

- Deployments
- Pods
- DaemonSets
- Service: ExternalName only (w.i.p)
- Ingress (w.i.p)

Currently uses vendored ssh, plan is to move to openssh and use the native binary on the host.

Supported Distro: Ubuntu 24.04
Supported architectures: amd64, arm64

## Architecture

- `skate` cli that is basically the scheduler, run from developers machine.
- talks to `skatelet` binaries on each host (not long lived agents, also a cli) over ssh

Could be described as one-shot scheduling.

## Registering nodes

```shell
skate create node --name foo --host bar
```

This will ensure all hosts are provisioned with `skatelet`, the agent

## Playing with objects

```shell
skate get pods

skate describe pod foo

skate get nodes

skate describe node bar

skate get deployments

skate describe deployment baz
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

### Ubuntu

```shell
# multipass image doesn't have much
sudo apt-get install -y gcc make libssl-dev pkg-config
```

### Features

- [x] Scheduling
    - Strategies
        - [x] Recreate
        - [ ] Rolling Deployments
    - [x] Pods
    - [x] Deployments
    - [x] Daemonsets
- Networking
    - [x] multi-host container network
    - [x] container dns
    - [ ] ingress
    - [ ] modded fanout to wait for all and round robin all

### Networking

Dns is coredns with fanout between all nodes along with serving from file.

Hosts are maintained via a CNI plugin that adds/removes the ip to the hosts file.

Good enough.
