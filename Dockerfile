# JobHunt Cloud: one image, several process modes (see .railway/railway.ts):
#
#   jobhunt server                  HTTP API + hosted MCP
#   jobhunt migrate                 database migrations (pre-deploy)
#   jobhunt worker discovery        scheduled discovery (cron)
#   jobhunt worker verification     scheduled re-verification (cron)
#
# Configuration comes from environment variables (README: "JobHunt Cloud").

FROM rust:1.94-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p jobhunt-cli \
    && strip target/release/jobhunt

# glibc, libgcc and CA certificates, no shell or package manager, runs as
# an unprivileged user.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/jobhunt /usr/local/bin/jobhunt
COPY deploy/cloud.toml /app/cloud.toml
WORKDIR /app
ENV JOBHUNT_CONFIG=/app/cloud.toml \
    RUST_BACKTRACE=1
EXPOSE 8080
CMD ["jobhunt", "server"]
