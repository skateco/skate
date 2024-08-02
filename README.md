# Skate

Sort of Kubernetes...

An extremely low footprint mini paas for scheduling resources on a small number of hosts.

**Kubernetes manifest compatible.**
Will support only a subset of resources and only a subset of their functionality:

- Deployments
- Pods
- DaemonSets
- Service: ExternalName only (w.i.p)
- Ingress
- Secrets
- CronJobs

Currently uses vendored ssh, plan is to move to openssh and use the native binary on the host.

Supported Distro: Ubuntu 24.04
Supported architectures: amd64, arm64

## Architecture

- `skate` cli that is basically the scheduler, run from developers machine.
- talks to `skatelet` binaries on each host (not long lived agents, also a cli) over ssh

Could be described as one-shot scheduling.

### Networking

Static routes between hosts, maintained by a systemd unit file.
All containers attached to the default `podman` network which we modify.

### DNS

Dns is coredns with fanout between all nodes along with serving from file.

Hosts are maintained via a CNI plugin that adds/removes the ip to the hosts file.

Pods get a hostname of `<labels.app>.<metadata.namespace>.cluster.skate.`

### Ingress

Nginx container listening on port 80 and 443

Use an Ingress resource to enable.

```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: foo-external
spec:
  rules:
  - host: foo.example.com
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: mypod.myns.cluster.skate
            port:
              number: 80
```

Service resources are ignored and it's implicit that a pod has a service with
url: `<labels.name>.<metadata.namespace>.cluster.skate`

Currently only Prefix pathType is supported.

### CronJobs

Uses systemd timers to schedule jobs.
Limited to always running on the same node.
Haven't looked in to the ForbidConcurrent etc yet. 
I 'think' systemd will just spawn a new job if they overlap.

### Secrets

Secrets are scheduled to all nodes for simplicity.
Any references to secrets in a pod manifest are automatically looked up in the same namespace as the pods.


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

### TODO

- Install
  - Supported distros/arch
    - [x] Ubuntu 24.04 amd64/aarch64
    - [ ] Raspbian armv7
  - [ ] Idempotent install

- Scheduling
    - Strategies
        - [x] Recreate
        - [ ] Rolling Deployments
    - Pods
        - [x] Apply
        - [ ] Remove
        - [x] List
    - Deployments
        - [x] Apply
        - [ ] Remove
        - [x] List
        - [ ] Output matches kubectl
    - Daemonsets
        - [x] Apply
        - [ ] Remove
        - [ ] List
        - [ ] Output matches kubectl
    - Ingress
        - [x] Apply
        - [x] Remove
        - [x] List
        - [x] Output matches kubectl
    -  Cron
        - [x] Apply
        - [x] Remove
        - [x] Hash checking
        - [x] List
        - [x] Output matches kubectl
    - Secret
        - [x] Apply
        - [x] Remove
        - [x] List
        - [ ] Output matches kubectl
        - Goal here is to support private image pull
    - ClusterIssuer
      - For letsencrypt

- Networking
    - [x] multi-host container network (currently static routes)
    - [ ] Use something fancier like vxlan
- DNS
    - [x] multi host dns
    - [x] ingress
    - [ ] modded fanout to wait for all and round robin all
    - [ ] something more barebones than coredns??
- Ingress
    - [x] Openresty config template from ingress resources
    - [x] letsencrypt
      - [ ] Cluster Issuer to set letsencrypt url
    - [ ] Support gateway api

