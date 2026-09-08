source <(cat the_plan2_fixed.txt | head -n 1 | sed 's/1. git rev-parse HEAD && //')
echo "${PR_BODY:0:50}"
