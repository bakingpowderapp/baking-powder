#!/bin/bash
cd "$(dirname "$0")" || exit 1
export LC_ALL=en_US.UTF-8

all=$(find . -path ./target -prune -o -path ./.git -prune -o -path ./exports -prune -o -path ./.claude/worktrees -prune -o -type f ! -name Cargo.lock ! -name .DS_Store -print | sed 's|^\./||' | sort)
factory=bp/src/factory
rest=$(grep -Ev "\.(rs|md|sh)$|^$factory/" <<< "$all")

glyphs=(█ ▓ ▒ ░ ·)
eighths=('' ▏ ▎ ▍ ▌ ▋ ▊ ▉)
kinds=() names=() lines=() chars=() pcts=() units=() group_chars=() segs=()
total_lines=0 total_chars=0 total_files=0 longest=0 name_w=0 fold=''

row() {
    kinds[${#kinds[@]}]=$1; names[${#names[@]}]=$2; lines[${#lines[@]}]=$3; chars[${#chars[@]}]=$4
}

group() {
    local label=$1 f l c d prev name at count=0 i sum_lines=0 sum_chars=0 n=0 under
    local fs=() fl=() fc=() fd=() dirs=() dl=() dc=() dn=()
    shift
    for f in $(for f in "$@"; do printf '%s\t%s\n' "$(dirname "$f")" "$f"; done | sort | cut -f2); do
        l=$(($(wc -l < "$f"))); c=$(($(wc -m < "$f")))
        if [ -n "$fold" ]; then d=${f#"$fold/"}; d=$fold/${d%%/*}; else case $f in */*) d=${f%/*} ;; *) d=. ;; esac; fi
        fs[$count]=$f; fl[$count]=$l; fc[$count]=$c; fd[$count]=$d; count=$((count + 1))
        if [ "$d" != "$prev" ]; then dirs[${#dirs[@]}]=$d; dl[${#dl[@]}]=0; dc[${#dc[@]}]=0; dn[${#dn[@]}]=0; prev=$d; fi
        at=$((${#dirs[@]} - 1))
        dl[$at]=$((dl[at] + l)); dc[$at]=$((dc[at] + c)); dn[$at]=$((dn[at] + 1))
        [ "$c" -gt "$longest" ] && longest=$c
        sum_lines=$((sum_lines + l)); sum_chars=$((sum_chars + c)); n=$((n + 1))
    done
    prev=''; at=-1; i=0
    while [ $i -lt $count ]; do
        d=${fd[$i]}
        if [ "$d" != "$prev" ]; then
            at=$((at + 1)); prev=$d; under=0
            if [ "$d" != . ] && { [ -n "$fold" ] || [ "${dn[$at]}" -gt 1 ]; }; then
                row dir "$d/" "${dl[$at]}" "${dc[$at]}"; under=1
                [ $((${#d} + 1)) -gt "$name_w" ] && name_w=$((${#d} + 1))
            fi
        fi
        if [ -z "$fold" ]; then
            name=${fs[$i]}
            [ $under = 1 ] && name="  ${name##*/}"
            row file "$name" "${fl[$i]}" "${fc[$i]}"
            [ ${#name} -gt "$name_w" ] && name_w=${#name}
        fi
        i=$((i + 1))
    done
    row total "${glyphs[${#group_chars[@]}]} $label ($n files)" "$sum_lines" "$sum_chars"
    row blank '' 0 0
    group_chars[${#group_chars[@]}]=$sum_chars
    total_lines=$((total_lines + sum_lines)); total_chars=$((total_chars + sum_chars)); total_files=$((total_files + n))
}

group Rust $(grep '\.rs$' <<< "$all")
group Markdown $(grep '\.md$' <<< "$all")
group Scripts $(grep '\.sh$' <<< "$all")
fold=$factory group Presets $(grep "^$factory/" <<< "$all")
group Config $rest
row stack "everything ($total_files files)" "$total_lines" "$total_chars"

row blank '' 0 0
for crate in bp coresynth fx sdk mac; do
    crate_files=$(grep -E "^$crate/src/.*\.rs$" <<< "$all")
    [ -n "$crate_files" ] || continue
    read -r crate_lines crate_chars < <(cat $crate_files | wc -lm)
    row total "$crate/" "$crate_lines" "$crate_chars"
done

cols=$(tput cols 2>/dev/null)
[ "$cols" -gt 0 ] 2>/dev/null || cols=80
animate=0
[ -t 1 ] && animate=1
bar_w=$((cols - 27 - 2 - name_w - 1))
[ "$bar_w" -gt 40 ] && bar_w=40
[ "$bar_w" -lt 12 ] && { bar_w=12; animate=0; }

i=0 sum=0 big=0
while [ $i -lt ${#group_chars[@]} ]; do
    segs[$i]=$(( (group_chars[i] * bar_w + total_chars / 2) / total_chars ))
    [ "${group_chars[$i]}" -gt 0 ] && [ "${segs[$i]}" -eq 0 ] && segs[$i]=1
    sum=$((sum + segs[i]))
    [ "${group_chars[$i]}" -gt "${group_chars[$big]}" ] && big=$i
    i=$((i + 1))
done
segs[$big]=$((segs[big] + bar_w - sum))

n=${#kinds[@]}
i=0
while [ $i -lt $n ]; do
    p=$(( (chars[i] * 1000 + total_chars / 2) / total_chars ))
    units[$i]=$(( (chars[i] * bar_w * 8 + longest / 2) / longest ))
    printf -v p '%d.%d%%' $((p / 10)) $((p % 10))
    pcts[$i]=$p
    i=$((i + 1))
done

bar() {
    local full=$(($1 / 8)) rem=$(($1 % 8)) s
    printf -v s '%*s' "$full" ''
    s=${s// /█}${eighths[$rem]}
    printf -v BAR '%s%*s' "$s" $((bar_w - full - (rem > 0))) ''
}

stack() {
    local n=$1 i=0 take s='' seg
    while [ $i -lt ${#segs[@]} ]; do
        take=${segs[$i]}
        [ "$take" -gt "$n" ] && take=$n
        printf -v seg '%*s' "$take" ''
        s=$s${seg// /${glyphs[$i]}}
        n=$((n - take)); i=$((i + 1))
    done
    printf -v BAR '%s%*s' "$s" $((bar_w - $1 + n)) ''
}

W=6
draw() {
    local i=$1 k=$2
    case ${kinds[$i]} in
        blank) printf '%s\n' "$cr" ;;
        total) printf "%s%'7d %'10d %6s  %s\n" "$cr" "${lines[$i]}" "${chars[$i]}" "${pcts[$i]}" "${names[$i]}" ;;
        dir) printf "%s%'7d %'10d %6s  %-*s  %s\n" "$cr" "${lines[$i]}" "${chars[$i]}" "${pcts[$i]}" "$bar_w" '' "${names[$i]}" ;;
        file) bar $((units[i] * k / W)); printf "%s%'7d %'10d %6s  %s  %s\n" "$cr" "${lines[$i]}" "${chars[$i]}" "${pcts[$i]}" "$BAR" "${names[$i]}" ;;
        stack) stack $((bar_w * k / W)); printf "%s%'7d %'10d %6s  %s  %s\n" "$cr" "${lines[$i]}" "${chars[$i]}" "${pcts[$i]}" "$BAR" "${names[$i]}" ;;
    esac
}

printf '%s\n\n' "Baking Powder — lines and characters per file, sized by characters (the nearer measure of tokens)"
printf '%7s %10s %6s  %-*s  %s\n' lines chars share "$bar_w" 'vs largest' file
if [ $animate = 1 ]; then
    cr=$'\r'
    printf '\033[?25l'
    trap 'printf "\033[?25h"' EXIT
    t=0
    while [ $t -lt $((n + W - 1)) ]; do
        first=$((t - W + 1)); [ $first -lt 0 ] && first=0
        last=$t; [ $last -ge $n ] && last=$((n - 1))
        [ $t -lt $n ] && printf '\n'
        printf '\033[%dA' $((last - first + 1))
        j=$first
        while [ $j -le $last ]; do draw $j $((t - j + 1)); j=$((j + 1)); done
        sleep 0.006
        t=$((t + 1))
    done
    printf '\033[?25h'
else
    cr=''
    i=0
    while [ $i -lt $n ]; do draw $i $W; i=$((i + 1)); done
fi
