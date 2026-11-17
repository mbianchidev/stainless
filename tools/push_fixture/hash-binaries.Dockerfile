ARG SERVER
ARG BUILDER
FROM ${SERVER} AS server
FROM ${BUILDER} AS hash
COPY --from=server /bin/autoconnect /bin/autoendpoint /fixture/
RUN sha256sum /fixture/autoconnect /fixture/autoendpoint \
    && wc -c /fixture/autoconnect /fixture/autoendpoint
