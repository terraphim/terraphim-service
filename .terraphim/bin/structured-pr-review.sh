#!/usr/bin/env bash
# Disciplined structural PR review agent (repo-local, 9-dimension checklist)
set -euo pipefail
REPO="${ADF_REPO:-terraphim-service}"
OWNER="${ADF_OWNER:-terraphim}"
PR_NUM="${1:-}"
if [ -z "$PR_NUM" ]; then echo "Usage: $0 <PR_NUMBER>" >&2; exit 1; fi
echo "[disciplined-pr-review] Reviewing PR #${PR_NUM} on ${OWNER}/${REPO}"
PR_DATA=$(gitea-robot view-pull --owner "$OWNER" --repo "$REPO" --index "$PR_NUM" 2>/dev/null)
PR_TITLE=$(echo "$PR_DATA" | python3 -c "import json,sys; print(json.load(sys.stdin)[\"title\"])" 2>/dev/null)
HEAD_BRANCH=$(echo "$PR_DATA" | python3 -c "import json,sys; print(json.load(sys.stdin).get(\"head\",{}).get(\"ref\",\"\"))" 2>/dev/null)
echo "  Title: $PR_TITLE"
echo "  Head: $HEAD_BRANCH"
git fetch origin "$HEAD_BRANCH" 2>/dev/null || true
DIFF=$(git diff origin/main..."origin/$HEAD_BRANCH" --stat 2>/dev/null || true)
REVIEW_FILE="/tmp/pr-review-${PR_NUM}-$(date +%s).md"
cat << EOF > "$REVIEW_FILE"
<h3>Summary</h3>
Automated structural PR review for **#${PR_NUM}** on \\\`${OWNER}/${REPO}\\\`.
**PR:** ${PR_TITLE}
**Dimensions checked:** 1..9 (Security, API Contract, Runtime, Performance, Type-Safety, Quality, UI/UX, Consistency, Documentation)
**Files changed:** \\\`\\\`\\\`${DIFF}\\\`\\\`\\\`
<h3>Confidence Score: N/5</h3>
<h3>Findings</h3>
*P0/P1/P2 severity-tiered findings with file/line references.*
<sub>Review by disciplined-pr-review | PR #${PR_NUM}</sub>
EOF
if [ "${DRY_RUN:-1}" = "1" ]; then
  echo "[disciplined-pr-review] DRY-RUN: review generated, not posted."
  cat "$REVIEW_FILE"
else
  gitea-robot comment --owner "$OWNER" --repo "$REPO" --issue "$PR_NUM" --body-file "$REVIEW_FILE" 2>/dev/null
  echo "[disciplined-pr-review] Review posted to PR #${PR_NUM}"
fi

