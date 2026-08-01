FROM debian:stable-slim

RUN mkdir -p /home/stowaway /usr/local/bin \
    && printf '#!/bin/sh\n[ "$1" = -n ] && shift\n[ "$1" = -- ] && shift\nexec "$@"\n' \
        > /usr/local/bin/sudo \
    && chmod 0755 /usr/local/bin/sudo

CMD ["sleep", "infinity"]
