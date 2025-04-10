FROM rust:1.86.0-bookworm AS build-env
LABEL maintainer="yanorei32"

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

WORKDIR /usr/src
COPY . /usr/src/merge-pr-in-all/
WORKDIR /usr/src/merge-pr-in-all
RUN cargo build --release && cargo install cargo-license && cargo license \
	--authors \
	--do-not-bundle \
	--avoid-dev-deps \
	--avoid-build-deps \
	--filter-platform "$(rustc -vV | sed -n 's|host: ||p')" \
	> CREDITS

FROM debian:bookworm-slim@sha256:4b44499bc2a6c78d726f3b281e6798009c0ae1f034b0bfaf6a227147dcff928b

RUN apt-get update; \
	apt-get install -y --no-install-recommends \
		libssl3 ca-certificates; \
	apt-get clean;

WORKDIR /

COPY --chown=root:root --from=build-env \
	/usr/src/merge-pr-in-all/CREDITS \
	/usr/src/merge-pr-in-all/LICENSE \
	/usr/share/licenses/merge-pr-in-all/

COPY --chown=root:root --from=build-env \
	/usr/src/merge-pr-in-all/target/release/merge-pr-in-all \
	/usr/bin/merge-pr-in-all

CMD ["/usr/bin/merge-pr-in-all"]
