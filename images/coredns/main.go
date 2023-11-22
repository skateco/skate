package main

// This file purely exists to tell go our requirements
// This enables us to use things like go mod tidy

import (
	"github.com/coredns/coredns/core/dnsserver"
	_ "github.com/coredns/coredns/core/plugin"
	"github.com/coredns/coredns/coremain"
	_ "github.com/openshift/coredns-mdns"
)

func init() {
	dnsserver.Directives = append(dnsserver.Directives, "mdns")
}

func main() {
	coremain.Run()
}
