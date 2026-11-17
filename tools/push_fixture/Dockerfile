ARG BUILDER
FROM ${BUILDER} AS build
SHELL ["/bin/bash", "-euo", "pipefail", "-c"]
WORKDIR /source
COPY . .
RUN mkdir -p /opt/cmake \
    && tar -xzf .fixture-tools/cmake.tar.gz -C /opt/cmake --strip-components=1
ENV CARGO_BUILD_JOBS=2 PATH="/opt/cmake/bin:${PATH}"
RUN cargo build --locked --release --jobs 2 --no-default-features --features postgres \
      --package autoconnect --package autoendpoint
RUN mkdir -p /runtime/bin /runtime/etc/ssl/certs \
    && cp target/release/autoconnect target/release/autoendpoint /runtime/bin/ \
    && ldd target/release/autoconnect target/release/autoendpoint > /runtime-linkage.txt \
    && ! grep -q 'not found' /runtime-linkage.txt \
    && awk '/=> \// {print $3} /^[[:space:]]*\// {print $1}' /runtime-linkage.txt \
       | sort -u | xargs -r cp --parents -t /runtime \
    && cp /etc/ssl/certs/ca-certificates.crt /runtime/etc/ssl/certs/ \
    && cp LICENSE version.json /runtime/

FROM scratch
COPY --from=build /runtime/ /
USER 10001:10001
WORKDIR /
STOPSIGNAL SIGTERM
