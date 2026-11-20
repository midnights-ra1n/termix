# termix-sync: build from the repository root (the web UI embeds assets/icon-rounded.svg):
#   docker build -f sync-server/Dockerfile -t termix-sync .
FROM rust:1-bookworm AS build
WORKDIR /src
COPY assets/icon-rounded.svg assets/icon-rounded.svg
COPY sync-server sync-server
RUN cd sync-server && cargo build --release --locked && mkdir -p /data

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/sync-server/target/release/termix-sync /usr/local/bin/termix-sync
COPY --from=build --chown=65532:65532 /data /data
ENV TERMIX_DB=/data/termix-sync.db TERMIX_LISTEN=0.0.0.0:8080
VOLUME /data
EXPOSE 8080
USER nonroot
ENTRYPOINT ["/usr/local/bin/termix-sync"]
