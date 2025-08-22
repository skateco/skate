package main

// This file purely exists to tell go our requirements
// This enables us to use things like go mod tidy

import (
	"slices"

	"github.com/coredns/coredns/core/dnsserver"
	_ "github.com/coredns/coredns/core/plugin"
	"github.com/coredns/coredns/coremain"
	// 	_ "github.com/networkservicemesh/fanout"
	// 	_ "github.com/openshift/coredns-mdns"
	_ "github.com/ziollek/gathersrv"
)

func init() {

	index := 0
	for i, plugin := range dnsserver.Directives {
		if plugin == "prometheus" {
			index = i
			break
		}
	}

	dnsserver.Directives = slices.Insert(dnsserver.Directives, index+1, "gathersrv")
}

func main() {
	coremain.Run()
}
