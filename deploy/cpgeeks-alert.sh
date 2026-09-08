#!/usr/bin/env bash
# Mails when a unit fails. Invoked by systemd's OnFailure=, never by hand.
#
# Uses the Resend key the application already has, so there is no second service
# to sign up for and no extra credential to leak. Deliberately best-effort: an
# alert that cannot be sent must not itself become a failing unit, or a single
# outage turns into a loop of units failing to report units failing.
set -uo pipefail

UNIT="${1:-unknown}"
ENV_FILE=/etc/cpgeeks/backend.env

# 2>/dev/null so a missing env file produces one clear message below rather
# than a burst of raw sed errors in front of it
read_env() { sed -n "s/^$1=//p" "$ENV_FILE" 2>/dev/null | head -1 | sed 's/^"//; s/"$//'; }

API_KEY=$(read_env RESEND_API_KEY)
FROM=$(read_env RESEND_FROM_EMAIL)
TO=$(read_env ALERT_EMAIL)

if [ -z "$API_KEY" ] || [ -z "$TO" ]; then
    echo "alert for $UNIT not sent: RESEND_API_KEY or ALERT_EMAIL missing" >&2
    exit 0
fi

: "${FROM:=SUST CP Geeks <noreply@mail.sustcpgeeks.me>}"

HOST=$(hostname)
WHEN=$(date -u '+%Y-%m-%d %H:%M:%S UTC')
LOGS=$(journalctl -u "$UNIT" -n 40 --no-pager 2>/dev/null || echo "(could not read logs)")

# Build the JSON in python rather than by string interpolation: log lines
# contain quotes, backslashes and newlines, and hand-built JSON breaks on the
# exact noisy failure you most want to be told about.
PAYLOAD=$(FROM="$FROM" TO="$TO" UNIT="$UNIT" HOST="$HOST" WHEN="$WHEN" LOGS="$LOGS" python3 - <<'PY'
import json, os
print(json.dumps({
    "from": os.environ["FROM"],
    "to": [os.environ["TO"]],
    "subject": f"[cpgeeks] {os.environ['UNIT']} failed on {os.environ['HOST']}",
    "text": (
        f"{os.environ['UNIT']} entered a failed state.\n\n"
        f"Host: {os.environ['HOST']}\n"
        f"Time: {os.environ['WHEN']}\n\n"
        "Last 40 log lines:\n\n"
        f"{os.environ['LOGS']}\n"
    ),
}))
PY
)

CODE=$(curl -s -o /dev/null -w '%{http_code}' --max-time 20 \
    -X POST https://api.resend.com/emails \
    -H "Authorization: Bearer $API_KEY" \
    -H "Content-Type: application/json" \
    -d "$PAYLOAD" 2>/dev/null || echo "000")

# never echo the key, and never fail: this runs *because* something already broke
if [ "$CODE" = "200" ]; then
    echo "alert sent for $UNIT"
else
    echo "alert for $UNIT could not be sent (resend http $CODE)" >&2
fi
exit 0
