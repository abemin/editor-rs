#!/bin/bash
# Fails if forbidden third-party material or secrets are tracked in this repository.
set -u
cd "$(dirname "$0")/.."
self="scripts/check-clean.sh"
status=0
files=$(git ls-files --cached --others --exclude-standard | grep -v "^$self$")
word="text""mate"
while IFS= read -r f; do
  [ -f "$f" ] || continue
  case "$f" in
    *.tmLanguage|*.tmTheme|*.tmbundle|*.tmPreferences) echo "forbidden file type: $f"; status=1 ;;
  esac
  if printf '%s' "$f" | grep -qi "$word"; then echo "forbidden name in path: $f"; status=1; fi
  if LC_ALL=C grep -Iqi "$word" -- "$f"; then echo "forbidden word in: $f"; status=1; fi
  if LC_ALL=C grep -IqE '^private_key=[A-Za-z0-9+/]{16,}' -- "$f"; then echo "secret value in: $f"; status=1; fi
done <<< "$files"
[ $status -eq 0 ] && echo "check-clean: ok"
exit $status
