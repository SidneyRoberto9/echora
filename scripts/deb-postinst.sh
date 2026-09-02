#!/bin/sh
set -e

# Debian's own icon packages call gtk-update-icon-cache/update-desktop-database
# in maintainer scripts (see dh_icons) because desktop environments consult a
# prebuilt icon-theme.cache instead of scanning the filesystem live. Without
# this, echora's icon (installed into /usr/share/icons/hicolor/*/apps/) is
# invisible to the shell until something else happens to rebuild the cache —
# confirmed locally: a fresh install showed a generic fallback icon in the
# dock until this ran. `|| true` on each: neither tool is guaranteed present
# on every distro, and a missing icon cache is cosmetic, not worth failing
# the whole install over.
gtk-update-icon-cache -f -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
update-desktop-database -q /usr/share/applications >/dev/null 2>&1 || true

exit 0
