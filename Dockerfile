FROM rust:1.85.0-bookworm AS build-env
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

FROM debian:bookworm-slim@sha256:44bccdd61bf09a081b1db8c61cf49bfabf30ac7afcc970010137c0ab587b209c

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
