FROM rust:alpine AS build


WORKDIR /
COPY ./ ./http-dump
WORKDIR /http-dump

# build for release
RUN cargo install --path .

# our final base
FROM alpine:3.23

LABEL authors="nbis-dev"

# copy the build artifact from the build stage
COPY --from=build /http-dump/target/release/http-dump .

EXPOSE 8089
# set the startup command to run your binary
CMD ["./http-dump"]

