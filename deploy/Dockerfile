ARG DEBIAN_IMAGE=debian:trixie-slim
FROM ${DEBIAN_IMAGE} AS base
ARG DEBIAN_MIRROR=deb.debian.org
RUN sed -i "s|deb.debian.org|${DEBIAN_MIRROR}|g" /etc/apt/sources.list.d/debian.sources
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates libgmp10 curl util-linux && rm -rf /var/lib/apt/lists/*
COPY bin/chublog-api /usr/local/bin/chublog-api
COPY gitconfig /etc/chublog/gitconfig
RUN groupadd -g 10002 ingress && useradd -u 10001 -g ingress -M -s /usr/sbin/nologin blog

FROM base AS api
USER 10001:10002
ENV API_BIND=0.0.0.0:3001 DATA_DIR=/data CHUBLOG_SSH_CONFIG=/auth/publishers.json INGRESS_DIR=/srv/ingress/authors RUST_LOG=info
ENTRYPOINT ["chublog-api"]

FROM base AS ssh
RUN apt-get update && apt-get install -y --no-install-recommends openssh-server && rm -rf /var/lib/apt/lists/* && useradd -u 10002 -g ingress -M -d /srv/ingress -s /bin/sh git && passwd -d git
COPY sshd_config /etc/ssh/sshd_config
COPY hooks /hooks
COPY ssh-entrypoint.sh /usr/local/bin/ssh-entrypoint
COPY authorized-keys.sh /usr/local/bin/chublog-authorized-keys
RUN chmod 755 /hooks/pre-receive /usr/local/bin/ssh-entrypoint /usr/local/bin/chublog-authorized-keys
ENTRYPOINT ["/usr/local/bin/ssh-entrypoint"]

FROM base AS init
COPY init-volumes.sh /usr/local/bin/init-volumes
RUN chmod 755 /usr/local/bin/init-volumes
ENTRYPOINT ["/usr/local/bin/init-volumes"]

FROM ${DEBIAN_IMAGE} AS database
ARG DEBIAN_MIRROR=deb.debian.org
RUN sed -i "s|deb.debian.org|${DEBIAN_MIRROR}|g" /etc/apt/sources.list.d/debian.sources
RUN apt-get update && apt-get install -y --no-install-recommends libgmp10 ca-certificates && rm -rf /var/lib/apt/lists/* && useradd -u 10003 -M -s /usr/sbin/nologin chusql
COPY bin/chusql-server bin/csql-bootstrap bin/libchusql_core_storage.so /usr/local/bin/
COPY chusql.toml /etc/chusql/settings.toml
COPY db-entrypoint.sh /usr/local/bin/db-entrypoint
RUN chmod 755 /usr/local/bin/db-entrypoint
ENV LD_LIBRARY_PATH=/usr/local/bin
USER 10003:10003
ENTRYPOINT ["/usr/local/bin/db-entrypoint"]
