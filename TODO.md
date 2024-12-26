### TODO

- Install
    - [ ] More idempotent install
    - [ ] Separate peer/private ip for each node.

      Since we use ssh, we need the client reachable ip to communicate, but then we need the private ip for each node to
      reach the other nodes dns
- Scheduling
    - [ ] Respect terminationGracePeriodSeconds when killing pods.
      Possibly already done by podman.
    - [ ] Deployment, Daemonset labels arent respected, need to somehow be added to pods, perhaps prefixed
- Pods
    - [ ] Store manifest in store so CNI plugin can get access
- CronJob
    - [ ] logs for cronjobs across runs
    - [ ] ForbidConcurrent
- Secret
    - [ ] Support private registry secrets
        - WONTFIX: This is done in k8s by attaching the secret to the default service account, or by adding the
          secret
          to the pod manifest. Since we don't want to have to deal with creating service accounts, and since podman
          kube play doesn't support imagePullSecrets, one has to login to the registry manually per node.
    -
- ClusterIssuer
    - [ ] email
- Volumes
    - [ ] Create path on host if it doesn't exist like docker (maybe there's a flag for that).

- Networking
    - [ ] Debug why setting up routes again breaks existing container -> route
        - Most likely to do with force deleting the podman network
    - [ ] Vpn like wireguard etc
- DNS
    - [ ] something more barebones than coredns??
    - [ ] Leave resolvd in place: dns should fallback to host dns
- Ingress
    - [ ] Support gateway api
    - [ ] Recreate & fix whatever breaks the sighup reload.
- OCI
    - [ ] Get pod config from store and not podman
- ConfigMap
    - [ ] Store on disk and load via kube play --configmap

- Service
    - [ ] use quorum_up and quorum_down in keepalived to toggle a 503 in ingress.

- [ ] Skate in podman/docker SIND. Copy Kinder node.
