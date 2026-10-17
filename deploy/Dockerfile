ARG DEBIAN_IMAGE=mirror.ccs.tencentyun.com/library/debian:trixie-slim
ARG RUST_IMAGE=mirror.ccs.tencentyun.com/library/rust:1-trixie

ARG HASKELL_IMAGE=mirror.ccs.tencentyun.com/library/haskell:9.10.3-slim-bookworm

FROM ${HASKELL_IMAGE} AS haskell-toolchain

FROM ${RUST_IMAGE} AS rust-toolchain
ENV CARGO_BUILD_JOBS=2
ARG CARGO_MIRROR_INDEX=sparse+https://rsproxy.cn/index/
RUN if [ -n "$CARGO_MIRROR_INDEX" ]; then \
      printf '[source.crates-io]\nreplace-with = "build-mirror"\n[source.build-mirror]\nregistry = "%s"\n' \
      "$CARGO_MIRROR_INDEX" > /usr/local/cargo/config.toml; \
    fi

FROM rust-toolchain AS rust-build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY apps/api ./apps/api
COPY external/ChuSQL-RustDriver ./external/ChuSQL-RustDriver
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --locked --release --package chublog-api --bin chublog-api && mkdir /out && cp target/release/chublog-api /out/

FROM rust-toolchain AS database-build
ARG DEBIAN_MIRROR=mirrors.tencentyun.com
RUN sed -i "s|deb.debian.org|${DEBIAN_MIRROR}|g" /etc/apt/sources.list.d/debian.sources \
    && apt-get update && apt-get install -y --no-install-recommends \
    binutils-gold pkg-config libgmp-dev libffi-dev zlib1g-dev libnuma-dev libtinfo6 \
    && rm -rf /var/lib/apt/lists/*
# GHC version must match the pinned ChuSQL Stack resolver.
COPY --from=haskell-toolchain /opt/ghc/9.10.3 /opt/ghc/9.10.3
COPY --from=haskell-toolchain /usr/local/bin/stack /usr/local/bin/stack
ENV PATH=/opt/ghc/9.10.3/bin:$PATH
WORKDIR /src/ChuSQL
COPY external/ChuSQL ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --manifest-path chusql-core/storage/Cargo.toml --locked --release --lib
ENV LD_LIBRARY_PATH=/src/ChuSQL/chusql-core/storage/target/release
RUN --mount=type=cache,target=/root/.stack \
    mkdir /out && cd chusql-server \
    && stack --system-ghc --no-install-ghc --no-terminal --jobs 2 build chusql-server:exe:chusql-server --no-test --no-bench --fast --copy-bins --local-bin-path /out \
    && cd ../chusql-bootstrap \
    && stack --system-ghc --no-install-ghc --no-terminal --jobs 2 build chusql-bootstrap:exe:csql-bootstrap --no-test --no-bench --fast --copy-bins --local-bin-path /out \
    && cp ../chusql-core/storage/target/release/libchusql_core_storage.so /out/

FROM ${DEBIAN_IMAGE} AS base
ARG DEBIAN_MIRROR=mirrors.tencentyun.com
RUN sed -i "s|deb.debian.org|${DEBIAN_MIRROR}|g" /etc/apt/sources.list.d/debian.sources
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates libgmp10 curl util-linux && rm -rf /var/lib/apt/lists/*
COPY --from=rust-build /out/chublog-api /usr/local/bin/chublog-api
COPY deploy/gitconfig /etc/chublog/gitconfig
RUN groupadd -g 10002 ingress && useradd -u 10001 -g ingress -M -s /usr/sbin/nologin blog

FROM base AS api
USER 10001:10002
ENV API_BIND=0.0.0.0:3001 DATA_DIR=/data CHUBLOG_SSH_CONFIG=/auth/publishers.json INGRESS_DIR=/srv/ingress/authors RUST_LOG=info
ENTRYPOINT ["chublog-api"]

FROM base AS ssh
RUN apt-get update && apt-get install -y --no-install-recommends openssh-server && rm -rf /var/lib/apt/lists/* && useradd -u 10002 -g ingress -M -d /srv/ingress -s /bin/sh git && passwd -d git
COPY deploy/sshd_config /etc/ssh/sshd_config
COPY deploy/hooks /hooks
COPY deploy/ssh-entrypoint.sh /usr/local/bin/ssh-entrypoint
COPY deploy/authorized-keys.sh /usr/local/bin/chublog-authorized-keys
RUN chmod 755 /hooks/pre-receive /usr/local/bin/ssh-entrypoint /usr/local/bin/chublog-authorized-keys
ENTRYPOINT ["/usr/local/bin/ssh-entrypoint"]

FROM base AS init
COPY deploy/init-volumes.sh /usr/local/bin/init-volumes
RUN chmod 755 /usr/local/bin/init-volumes
ENTRYPOINT ["/usr/local/bin/init-volumes"]

FROM ${DEBIAN_IMAGE} AS database
ARG DEBIAN_MIRROR=mirrors.tencentyun.com
RUN sed -i "s|deb.debian.org|${DEBIAN_MIRROR}|g" /etc/apt/sources.list.d/debian.sources
RUN apt-get update && apt-get install -y --no-install-recommends libgmp10 ca-certificates && rm -rf /var/lib/apt/lists/* && useradd -u 10003 -M -s /usr/sbin/nologin chusql
COPY --from=database-build /out/chusql-server /out/csql-bootstrap /out/libchusql_core_storage.so /usr/local/bin/
COPY deploy/chusql.toml /etc/chusql/settings.toml
COPY deploy/db-entrypoint.sh /usr/local/bin/db-entrypoint
RUN chmod 755 /usr/local/bin/db-entrypoint
ENV LD_LIBRARY_PATH=/usr/local/bin
RUN chusql-server --help && csql-bootstrap --help
USER 10003:10003
ENTRYPOINT ["/usr/local/bin/db-entrypoint"]
