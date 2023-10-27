# Skate

Sort of Kubernetes...

## Architecture

```puml
actor Human
package local {
[Human] -> [Skate]
}
package "remote host" {
[Skate] -> [Skatelet]: ssh
}
```

