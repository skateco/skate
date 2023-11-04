.PHONY: default
default: armv7

.PHONY: armv6
armv6: # for ras pi 1 NOTE: sometimes have to run it a few times to get the whole thing built ¯\_(ツ)_/¯
	cross build  --target arm-unknown-linux-gnueabi
.PHONY: armv7
armv7: # for ras pi 2, also have to rerun sometimes ¯\_(ツ)_/¯
	cross build  --target armv7-unknown-linux-gnueabi
