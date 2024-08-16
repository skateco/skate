# Skate

Daemonless low footprint self-hosted mini-paas with support for deploying kubernetes manifests.

Born out of the frustration of having to learn yet another deployment configuration file syntax.

Skate runs as a CLI on your machine and talks to a small binary on each host over ssh.

Leverages [podman kube play](https://docs.podman.io/en/latest/markdown/podman-kube-play.1.html) to run pod manifests.

Supported Distro: Ubuntu 24.04
Supported architectures: amd64, arm64

You can deploy:

- Pods
- Deployments
- DaemonSets
- CronJobs
- Ingress
- Secrets

An nginx ingress runs on port 80 and 443 on all nodes.
Lets-encrypt provides the certificates.

## Getting Started

To play around I suggest using [mulitpass](https://multipass.run/) to create a few Ubuntu VMs.
You can use [./hack/clusterplz](./hack/clusterplz) to create a cluster of 2 nodes easily using multipass.
Skate only supports private key authentication for now, so make sure your nodes are set up to allow you key.

```shell
./hack/clusterplz create
```

BTW: you can use `./hack/clusterplz restore` to restore a clean snapshot of the nodes if things get messed up.

Install the skate CLI:

```shell
# Get list of latest release binaries
curl -s https://api.github.com/repos/skateco/skate/releases/latest | grep "browser_download_url.*tar.gz" | cut -d : -f 2,3 | tr -d \\\" | tr -d "[:blank:]"|grep -v skatelet
```

Download the `skate` binary for your platform and architecture.

Put it in your path.

Now, let's register a cluster:

*Note: Change ~/.ssh/id_rsa to the path to the private key that can access your nodes*

```shell
skate create cluster my-cluster --default-user $USER --default-key ~/.ssh/id_rsa
```

Add the nodes:

```shell
> ./hack/clusterplz ips
192.168.76.11
192.168.76.12

# The --subnet-cidr has to be unique per node
> skate create node --name node-1 --host 192.168.76.11 --subnet-cidr 20.1.0.0/16
...
... much install

> skate create node --name node-2 --host 192.168.76.12 --subnet-cidr 20.2.0.0/16

...
... much install

```

Ok, now we should have a 2 node cluster that we can deploy to.

```shell
> skate get nodes
NAME                            PODS        STATUS    
node-1                          2           Healthy   
node-2                          2           Healthy  
```

Create a deployment

```shell
cat <<EOF | skate apply -f -
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: nginx
  namespace: my-app
spec:
  replicas: 2
  template:
    spec:
      containers:
      - name: nginx
        image: nginx:1.14.2
EOF
```

Check the deployment

```shell   
skate get deployment -n my-app
```

### Networking

Static routes between hosts, maintained by a systemd unit file.
All containers attached to the default `podman` network which we modify.

### DNS

Dns is coredns with fanout between all nodes along with serving from file.

Hosts are maintained via a CNI plugin that adds/removes the ip to the hosts file.

Pods get a hostname of `<labels.app>.<metadata.namespace>.cluster.skate.`

### Ingress

Nginx container listening on port 80 and 443

Use an Ingress resource to route traffic to a pod.

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

Supported annotations:

- [ ] `nginx.ingress.kubernetes.io/ssl-redirect`
- [x] `nginx.ingress.kubernetes.io/proxy-body-size`

#### Healthchecks

`podman kube play` supports `livenessProbe` in the pod manifest.
The best way to ensure that http traffic stops being routed to an unhealthy pod is to combine that with `restartPolicy`
of `Always` or `OnFailure`.

**Traffic will only start being routed to your pod once all containers in the pod are healthy.**

NOTE: using the `httpGet` probe results in podman trying to run `curl` within the container.
With `tcpSocket` it looks for `nc`.

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
# subnet-cidr has to be unique per node
skate create node --name node-1 --host 192.168.0.62 --subnet-cidr 20.1.0.0/16
skate create node --name node-2 --host 192.168.0.72 --subnet-cidr 20.2.0.0/16
```

This will ensure all hosts are provisioned with `skatelet`, the agent

## Viewing objects

```shell
skate get pods -n <namespace>
skate get deployments -n <namespace>
skate get cronjobs -n <namespace>
skate get ingress -n <namespace>
skate get secrets -n <namespace>
```

## Deploying manifests

```shell
skate apply -f manifest.yaml
```

## Developing

### Mac

Native:

Install the targets:

```shell
rustup target add x86_64-unknown-linux-gnu
rustup target add aarch64-unknown-linux-gnu
````

Install the cross toolchains:

```shell
brew tap messense/macos-cross-toolchains
# install x86_64-unknown-linux-gnu toolchain
brew install x86_64-unknown-linux-gnu
# install aarch64-unknown-linux-gnu toolchain
brew install aarch64-unknown-linux-gnu
```

```shell
make amd64
## or
make aarch64
```

Or just use [https://github.com/cross-rs/cross](https://github.com/cross-rs/cross)

```shell
make amd64-cross
## or
make aarch64-cross
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
        - [ ] Store manifest in store so CNI plugin can get access
        - [x] Fix pod naming to avoid collisions
        - [x] Logs
    - Deployments
        - [x] Apply
        - [x] Remove
        - [x] List
        - [x] Logs
        - [x] Output matches kubectl
    - Daemonsets
        - [x] Apply
        - [x] Remove
        - [x] List
        - [x] Logs
        - [x] Output matches kubectl
    - Ingress
        - [x] Apply
        - [x] Remove
        - [x] List
        - [x] Output matches kubectl
        - [x] Https redirect
            - [ ] Opt out with annotation: `nginx.ingress.kubernetes.io/ssl-redirect: "false"`
    - Cron
        - [x] Apply
        - [x] Remove
        - [x] Hash checking
        - [x] List
        - [x] Output matches kubectl
        - [x] Logs
        - [ ] ForbidConcurrent
        - [ ] Create the pod when creating the cronjob to check it's legit
    - Secret
        - [x] Apply
        - [x] Remove
        - [x] List
        - [x] Output matches kubectl
        - [ ] Support private registry secrets
            - WONTFIX: This is done in k8s by attaching the secret to the default service account, or by adding the
              secret
              to the pod manifest. Since we don't want to have to deal with creating service accounts, and since podman
              kube play doesn't support imagePullSecrets, one has to login to the registry manually per node.
        -
    - ClusterIssuer
        - [ ] Lets encrypt api endpoint
        - [ ] email

- Networking
    - [x] multi-host container network (currently static routes)
    - [ ] Debug why setting up routes again breaks existing container -> route
        - Most likely to do with force deleting the podman network
    - [ ] Use something fancier like vxlan, tailscale etc
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
    - [ ] Recreate & fix whatever breaks the sighup reload.
- CNI
    - [ ] Get pod config from store and not podman

