# SPDX-License-Identifier: Apache-2.0
#
# Copyright © 2017-2019 WireGuard LLC. All Rights Reserved.

DESTDIR ?= $(CURDIR)/../../build/lib/$(RUST_TARGET_TRIPLE)
ANDROID_PACKAGE_NAME ?= net.nymtech.nymvpn

NDK_GO_ARCH_MAP_x86 := 386
NDK_GO_ARCH_MAP_x86_64 := amd64
NDK_GO_ARCH_MAP_arm := arm
NDK_GO_ARCH_MAP_arm64 := arm64
NDK_GO_ARCH_MAP_mips := mipsx
NDK_GO_ARCH_MAP_mips64 := mips64x

export CGO_CFLAGS := $(CFLAGS)
export CGO_LDFLAGS := $(LDFLAGS)
export CC := $(ANDROID_C_COMPILER)
export GOARCH := $(NDK_GO_ARCH_MAP_$(ANDROID_ARCH_NAME))
export GOOS := android
export CGO_ENABLED := 1

default: $(DESTDIR)/libwg.so

$(DESTDIR)/libwg.so:
	mkdir -p $(DESTDIR)
	go get -tags "linux android"
	chmod -fR +w "$(GOPATH)/pkg/mod"
	go build -tags "linux android" \
		-ldflags="-X main.socketDirectory=/data/data/$(ANDROID_PACKAGE_NAME)/cache/wireguard -buildid=" \
		-trimpath \
		-buildvcs=false \
		-v \
		-o "$@" \
		-buildmode c-shared
	rm -f $(DESTDIR)/libwg.h


clean:
	rm -f $(DESTDIR)/libwg.so
