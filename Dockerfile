# JobHunt Cloud: one image, several process modes (see .railway/railway.ts):
#
#   narrow server                  HTTP API + hosted MCP
#   narrow migrate                 database migrations (pre-deploy)
#   narrow worker discovery        scheduled discovery (cron)
#   narrow worker verification     scheduled re-verification (cron)
#
# Configuration comes from environment variables (README: "JobHunt Cloud").

FROM rust:1.94-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p jobhunt-cli \
    && strip target/release/narrow

# glibc, libgcc and CA certificates, no shell or package manager, runs as
# an unprivileged user.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/narrow /usr/local/bin/narrow
COPY deploy/cloud.toml /app/cloud.toml
WORKDIR /app
ENV JOBHUNT_CONFIG=/app/cloud.toml \
    RUST_BACKTRACE=1
EXPOSE 8080
CMD ["narrow", "server"]
