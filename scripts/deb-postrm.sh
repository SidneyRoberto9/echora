#!/bin/sh
set -e

# Mirrors deb-postinst.sh: after removal, echora's icon files are gone from
# hicolor, so the cache needs rebuilding again or it keeps a dangling entry.
gtk-update-icon-cache -f -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
update-desktop-database -q /usr/share/applications >/dev/null 2>&1 || true

exit 0
