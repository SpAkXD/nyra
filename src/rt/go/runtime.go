// ---- Nyra runtime (Go) --------------------------------------------------------------------
// Ints are int64 and wrap, floats print like JavaScript's String(x), lengths and indexes of
// strings count characters (runes). Arrays are *Array[T]: a slice that is copied before a write
// when it is shared, so two variables never see each other's changes; structs are Go values.

const nyFile = @FILE@

// nyOut buffers what the program prints; main flushes it at the end, nyFail before the error.
var nyOut = bufio.NewWriterSize(os.Stdout, 1<<16)

// nyFail reports a Nyra runtime error after all earlier output and exits with 101.
func nyFail(code, msg, hint string, line, col int) {
	nyOut.Flush()
	if os.Getenv("NYRA_JSON") != "" {
		fmt.Fprintf(os.Stderr, "{\"ok\":false,\"errors\":[{\"code\":\"%s\",\"message\":%s,\"file\":%s,\"line\":%d,\"col\":%d,\"hint\":%s,\"runtime\":true}]}\n",
			code, nyJSON(msg), nyJSON(nyFile), line, col, nyJSON(hint))
	} else {
		fmt.Fprintf(os.Stderr, "runtime error[%s]: %s\n  --> %s:%d:%d\n  = hint: %s\n  = explain: nyra explain %s\n", code, msg, nyFile, line, col, hint, code)
	}
	os.Exit(101)
}

func nyJSON(s string) string {
	var b strings.Builder
	b.WriteByte('"')
	for _, c := range s {
		switch {
		case c == '"':
			b.WriteString("\\\"")
		case c == '\\':
			b.WriteString("\\\\")
		case c == '\n':
			b.WriteString("\\n")
		case c == '\r':
			b.WriteString("\\r")
		case c == '\t':
			b.WriteString("\\t")
		case c == '\b':
			b.WriteString("\\b")
		case c == '\f':
			b.WriteString("\\f")
		case c < 0x20:
			fmt.Fprintf(&b, "\\u%04x", c)
		default:
			b.WriteRune(c)
		}
	}
	b.WriteByte('"')
	return b.String()
}

func nyOOM(line, col int) {
	nyFail("E0249", "out of memory", "the program needs more memory than the system gave it", line, col)
}

// nyIf is `if c { a } else { b }` for values (both are already computed: they have no effects).
func nyIf[T any](c bool, a, b T) T {
	if c {
		return a
	}
	return b
}

// nyI64 and nyF64 keep Go from computing constant expressions exactly at compile time.
func nyI64(x int64) int64     { return x }
func nyF64(x float64) float64 { return x }

// ---- ints ----

// int + - * / and negation: a result outside the 64-bit range is a runtime error (E0255).
func nyOverflow(a int64, op string, b int64, line, col int) {
	msg := fmt.Sprintf("int overflow: %d %s %d does not fit in 64 bits", a, op, b)
	if op == "~" {
		msg = fmt.Sprintf("int overflow: -(%d) does not fit in 64 bits", a)
	}
	nyFail("E0255", msg, "an int holds -9223372036854775808 to 9223372036854775807: use smaller values, or keep a running value small with `%` (e.g. `h = (h * 31 + x) % 1000000007`)", line, col)
}

func nyAdd(a, b int64, line, col int) int64 {
	r := a + b
	if (a >= 0) == (b >= 0) && (r >= 0) != (a >= 0) {
		nyOverflow(a, "+", b, line, col)
	}
	return r
}

func nySub(a, b int64, line, col int) int64 {
	r := a - b
	if (a >= 0) != (b >= 0) && (r >= 0) != (a >= 0) {
		nyOverflow(a, "-", b, line, col)
	}
	return r
}

func nyMul(a, b int64, line, col int) int64 {
	r := a * b
	if a != 0 && (r/a != b || (a == -1 && b == math.MinInt64) || (b == -1 && a == math.MinInt64)) {
		nyOverflow(a, "*", b, line, col)
	}
	return r
}

func nyNeg(a int64, line, col int) int64 {
	if a == math.MinInt64 {
		nyOverflow(a, "~", 0, line, col)
	}
	return -a
}

func nyDiv(a, b int64, line, col int) int64 {
	if b == 0 {
		nyFail("E0241", "division by zero", "check the divisor first", line, col)
	}
	if b == -1 && a == math.MinInt64 {
		nyOverflow(a, "/", b, line, col)
	}
	return a / b
}

func nyRem(a, b int64, line, col int) int64 {
	if b == 0 {
		nyFail("E0241", "division by zero", "check the divisor first", line, col)
	}
	return a % b
}

// `for i in a..b step k`: a step of 0 would never end.
// nyCheckNonEmpty is `xs.min()` / `xs.max()` of an empty array (`n` elements seen).
func nyCheckNonEmpty(n, max int64, line, col int) {
	if n == 0 {
		msg := "min() of an empty array"
		if max != 0 {
			msg = "max() of an empty array"
		}
		nyFail("E0247", msg, "an empty array has no smallest or largest element: check `xs.len() > 0` first, or start from a value of your own with `fold`", line, col)
	}
}

func nyCheckStep(k int64, line, col int) {
	if k == 0 {
		nyFail("E0243", "range step must not be 0", "use a positive step to count up and a negative one to count down", line, col)
	}
}

// int(x) of a float: truncates toward zero; NaN or a value outside the int range is an error.
func nyF2I(x float64, line, col int) int64 {
	if x != x || x >= 9223372036854775807.0 || x < -9223372036854775808.0 {
		nyFail("E0245", "cannot convert "+nyNum(x)+" to int", "int(x) needs a float that is not NaN and fits in an int", line, col)
	}
	return int64(x)
}

// nyNum shows a float like JavaScript's String(x): the shortest digits that read back the same.
func nyNum(x float64) string {
	switch {
	case x != x:
		return "NaN"
	case x == 0:
		return "0"
	case math.IsInf(x, 1):
		return "Infinity"
	case math.IsInf(x, -1):
		return "-Infinity"
	}
	sign := ""
	if x < 0 {
		sign, x = "-", -x
	}
	e := strconv.FormatFloat(x, 'e', -1, 64) // "1.2345e+20"
	mant, exp, _ := strings.Cut(e, "e")
	digits := strings.Replace(mant, ".", "", 1)
	k := len(digits)
	p, _ := strconv.Atoi(exp)
	n := p + 1 // the value is 0.DIGITS * 10^n
	switch {
	case k <= n && n <= 21:
		return sign + digits + strings.Repeat("0", n-k)
	case 0 < n && n <= 21:
		return sign + digits[:n] + "." + digits[n:]
	case -6 < n && n <= 0:
		return sign + "0." + strings.Repeat("0", -n) + digits
	}
	m := digits[:1]
	if k > 1 {
		m += "." + digits[1:]
	}
	if n > 0 {
		return sign + m + "e+" + strconv.Itoa(n-1)
	}
	return sign + m + "e-" + strconv.Itoa(1-n)
}

// ---- strings: UTF-8, but lengths and indexes count characters ----

func nyLen(s string) int64 { return int64(utf8.RuneCountInString(s)) }

func nyOOB(i, n int64, line, col int) {
	nyFail("E0240", fmt.Sprintf("index %d is out of bounds for length %d", i, n), "valid indexes are 0 to len - 1; compare with `.len()` first", line, col)
}

func nyRange(a, b, n int64, line, col int) {
	if a < 0 || a > b || b > n {
		nyFail("E0240", fmt.Sprintf("range %d..%d is out of bounds for length %d", a, b, n), "a range a..b needs 0 <= a <= b <= len", line, col)
	}
}

func nyASCII(s string) bool {
	for i := 0; i < len(s); i++ {
		if s[i] >= 0x80 {
			return false
		}
	}
	return true
}

// s[i]
func nyCharAt(s string, i int64, line, col int) rune {
	if nyASCII(s) {
		if i < 0 || i >= int64(len(s)) {
			nyOOB(i, int64(len(s)), line, col)
		}
		return rune(s[i])
	}
	r := []rune(s)
	if i < 0 || i >= int64(len(r)) {
		nyOOB(i, int64(len(r)), line, col)
	}
	return r[i]
}

func nyStrSlice(s string, a, b int64, line, col int) string {
	if nyASCII(s) {
		nyRange(a, b, int64(len(s)), line, col)
		return s[a:b]
	}
	r := []rune(s)
	nyRange(a, b, int64(len(r)), line, col)
	return string(r[a:b])
}

// nyFind is the character position of t in s, or -1.
func nyFind(s, t string) int64 {
	j := strings.Index(s, t)
	if j < 0 {
		return -1
	}
	return nyLen(s[:j])
}

func nyReplace(s, old, new string, line, col int) string {
	if old == "" {
		nyFail("E0243", "replace() needs a non-empty pattern", "the text to replace can't be \"\"", line, col)
	}
	return strings.ReplaceAll(s, old, new)
}

// ASCII letters only, like every backend.
func nyUpper(s string) string {
	return strings.Map(func(c rune) rune { return nyCharUpper(c) }, s)
}

func nyLower(s string) string {
	return strings.Map(func(c rune) rune { return nyCharLower(c) }, s)
}

func nyRepeatStr(s string, n int64, line, col int) string {
	if n < 0 {
		nyFail("E0243", fmt.Sprintf("repeat count must be >= 0, got %d", n), "repeat(n) needs n >= 0", line, col)
	}
	// the longest text every backend can make (in UTF-8 bytes)
	if len(s) > 0 && n > 536870888/int64(len(s)) {
		nyOOM(line, col)
	}
	return strings.Repeat(s, int(n))
}

// `s.pad_left(n, c)` / `s.pad_right(n, c)`: `c` added until `s` has `n` characters.
func nyPad(s string, n int64, c rune, left bool) string {
	missing := n - nyLen(s)
	if missing <= 0 {
		return s
	}
	if missing > 536870888 {
		nyOOM(0, 0)
	}
	fill := strings.Repeat(string(c), int(missing))
	if left {
		return fill + s
	}
	return s + fill
}

// char tests: ASCII only, like upper() and lower()
func nyIsDigit(c rune) bool  { return c >= '0' && c <= '9' }
func nyIsUpper(c rune) bool  { return c >= 'A' && c <= 'Z' }
func nyIsLower(c rune) bool  { return c >= 'a' && c <= 'z' }
func nyIsLetter(c rune) bool { return nyIsUpper(c) || nyIsLower(c) }
func nyIsSpace(c rune) bool  { return c == ' ' || c == '\t' || c == '\n' || c == '\r' }

func nyCharUpper(c rune) rune {
	if nyIsLower(c) {
		return c - 32
	}
	return c
}

func nyCharLower(c rune) rune {
	if nyIsUpper(c) {
		return c + 32
	}
	return c
}

func nyChr(n int64, line, col int) rune {
	if n < 0 || n > 1114111 || (n >= 55296 && n <= 57343) {
		nyFail("E0246", fmt.Sprintf("char(%d): not a valid character code", n), "character codes go from 0 to 1114111, except 55296 to 57343", line, col)
	}
	return rune(n)
}

// nyShown is the text of a string in an error message: control characters as escapes.
func nyShown(s string) string {
	var b strings.Builder
	for _, c := range s {
		switch {
		case c == '\n':
			b.WriteString("\\n")
		case c == '\t':
			b.WriteString("\\t")
		case c == '\r':
			b.WriteString("\\r")
		case c < 0x20:
			fmt.Fprintf(&b, "\\u%04x", c)
		default:
			b.WriteRune(c)
		}
	}
	return b.String()
}

func nyDigits(s string) bool {
	if s == "" {
		return false
	}
	for i := 0; i < len(s); i++ {
		if s[i] < '0' || s[i] > '9' {
			return false
		}
	}
	return true
}

// int(s): digits with an optional `-` that fit in an int, nothing else.
func nyInt(s string, line, col int) int64 {
	if nyDigits(strings.TrimPrefix(s, "-")) {
		if v, err := strconv.ParseInt(s, 10, 64); err == nil {
			return v
		}
	}
	nyFail("E0244", "cannot parse \""+nyShown(s)+"\" as int", "int(s) accepts only digits with an optional `-`, e.g. \"-42\"", line, col)
	return 0
}

// float(s): -?[0-9]+(.[0-9]+)?([eE][+-]?[0-9]+)?
func nyFloat(s string, line, col int) float64 {
	t := strings.TrimPrefix(s, "-")
	whole, rest := t, ""
	if i := strings.IndexAny(t, ".eE"); i >= 0 {
		whole, rest = t[:i], t[i:]
	}
	ok := nyDigits(whole)
	if ok && strings.HasPrefix(rest, ".") {
		frac := rest[1:]
		rest = ""
		if i := strings.IndexAny(frac, "eE"); i >= 0 {
			frac, rest = frac[:i], frac[i:]
		}
		ok = nyDigits(frac)
	}
	if ok && rest != "" {
		exp := strings.TrimLeft(rest[1:], "+-")
		ok = (rest[0] == 'e' || rest[0] == 'E') && len(rest[1:])-len(exp) <= 1 && nyDigits(exp)
	}
	if !ok {
		nyFail("E0244", "cannot parse \""+nyShown(s)+"\" as float", "float(s) accepts digits with an optional `-`, `.` part and exponent, e.g. \"-1.5e3\"", line, col)
	}
	v, _ := strconv.ParseFloat(s, 64) // a value too big for a float is an infinity
	return v
}

func nyChars(s string) *Array[rune] { return &Array[rune]{items: []rune(s)} }

func nyCodes(s string) *Array[int64] {
	r := &Array[int64]{}
	for _, c := range s {
		r.items = append(r.items, int64(c))
	}
	return r
}

func nySplit(s, sep string, line, col int) *Array[string] {
	if sep == "" {
		nyFail("E0243", "split() needs a non-empty separator", "for the characters of a string use `s.chars()`", line, col)
	}
	return &Array[string]{items: strings.Split(s, sep)}
}

func nyJoin(xs *Array[string], sep string) string { return strings.Join(xs.items, sep) }

func nyJoinChars(xs *Array[rune], sep string) string {
	parts := make([]string, len(xs.items))
	for i, c := range xs.items {
		parts[i] = string(c)
	}
	return strings.Join(parts, sep)
}

// ---- arrays: copied on write ----

// Array is a Nyra array. A value with more than one owner is marked shared; a write to a
// shared array copies it first (see nyUnique).
type Array[T any] struct {
	items  []T
	shared bool
}

// nySharer is a value that holds arrays: an array, or a struct with array fields.
type nySharer interface{ nyShare() }

func (a *Array[T]) nyShare() { a.shared = true }

// nyArray makes a new array of the given elements.
func nyArray[T any](items ...T) *Array[T] { return &Array[T]{items: items} }

// nyShare marks a value that gets one more owner, and returns it.
func nyShare[T any](v T) T {
	if s, ok := any(v).(nySharer); ok {
		s.nyShare()
	}
	return v
}

// nyShareAll marks the elements of a new array shared (another array has them too).
func nyShareAll[T any](items []T) []T {
	for i := range items {
		s, ok := any(items[i]).(nySharer)
		if !ok {
			break
		}
		s.nyShare()
	}
	return items
}

// nyUnique is `a` itself when it has one owner, else a copy: what a write needs.
func nyUnique[T any](a *Array[T]) *Array[T] {
	if !a.shared {
		return a
	}
	return &Array[T]{items: nyShareAll(slices.Clone(a.items))}
}

// Map is a Nyra map: entries in insertion order (a removed one is a gap until the next
// compaction), an index from key to position, and the mark for a shared map.
type Map[K comparable, V any] struct {
	keys   []K
	vals   []V
	alive  []bool
	index  map[K]int
	live   int
	shared bool
}

func (m *Map[K, V]) nyShare() { m.shared = true }

// nyMOf makes a map of the keys and values, in order (a repeated key keeps its first place).
func nyMOf[K comparable, V any](keys []K, vals []V) *Map[K, V] {
	m := &Map[K, V]{index: map[K]int{}}
	for i := range keys {
		m.set(keys[i], vals[i])
	}
	return m
}

func (m *Map[K, V]) has(k K) bool {
	_, ok := m.index[k]
	return ok
}

func (m *Map[K, V]) set(k K, v V) {
	if i, ok := m.index[k]; ok {
		m.vals[i] = v
		return
	}
	m.index[k] = len(m.keys)
	m.keys = append(m.keys, k)
	m.vals = append(m.vals, v)
	m.alive = append(m.alive, true)
	m.live++
}

func (m *Map[K, V]) remove(k K) {
	i, ok := m.index[k]
	if !ok {
		return
	}
	delete(m.index, k)
	var zk K
	var zv V
	m.keys[i], m.vals[i], m.alive[i] = zk, zv, false
	m.live--
	if len(m.keys) > 8 && m.live < len(m.keys)/2 {
		c := nyMCopy(m)
		*m = *c
	}
}

// nyMCopy is a compact copy; the values it shares are marked shared.
func nyMCopy[K comparable, V any](m *Map[K, V]) *Map[K, V] {
	c := &Map[K, V]{index: make(map[K]int, m.live)}
	for i, k := range m.keys {
		if m.alive[i] {
			c.set(k, nyShare(m.vals[i]))
		}
	}
	return c
}

// nyMUnique is `m` itself when it has one owner, else a copy: what a write needs.
func nyMUnique[K comparable, V any](m *Map[K, V]) *Map[K, V] {
	if !m.shared {
		return m
	}
	return nyMCopy(m)
}

// nyMGet is m[k]: E0248 when the key is missing.
func nyMGet[K comparable, V any](m *Map[K, V], k K, line, col int) V {
	i, ok := m.index[k]
	if !ok {
		var b strings.Builder
		nyShowAny(&b, k)
		nyFail("E0248", "key "+b.String()+" is not in the map", "check with `m.has(k)` first, or read it with `m.get(k, default)`", line, col)
	}
	return m.vals[i]
}

// nyMSlot is the address of m[k], to change it in place: E0248 when the key is missing.
func nyMSlot[K comparable, V any](m *Map[K, V], k K, line, col int) *V {
	i, ok := m.index[k]
	if !ok {
		var b strings.Builder
		nyShowAny(&b, k)
		nyFail("E0248", "key "+b.String()+" is not in the map", "check with `m.has(k)` first, or read it with `m.get(k, default)`", line, col)
	}
	return &m.vals[i]
}

func nyMGetOr[K comparable, V any](m *Map[K, V], k K, d V) V {
	if i, ok := m.index[k]; ok {
		return m.vals[i]
	}
	return d
}

func nyMKeys[K comparable, V any](m *Map[K, V]) *Array[K] {
	out := make([]K, 0, m.live)
	for i, k := range m.keys {
		if m.alive[i] {
			out = append(out, k)
		}
	}
	return &Array[K]{items: out}
}

func nyMValues[K comparable, V any](m *Map[K, V]) *Array[V] {
	out := make([]V, 0, m.live)
	for i, v := range m.vals {
		if m.alive[i] {
			out = append(out, nyShare(v))
		}
	}
	return &Array[V]{items: out}
}

func (m *Map[K, V]) nyEq(o any) bool {
	n := o.(*Map[K, V])
	if m.live != n.live {
		return false
	}
	for i, k := range m.keys {
		if !m.alive[i] {
			continue
		}
		j, ok := n.index[k]
		if !ok || !nyEqual(m.vals[i], n.vals[j]) {
			return false
		}
	}
	return true
}

func (m *Map[K, V]) nyShowIn(b *strings.Builder) {
	if m.live == 0 {
		b.WriteString("[:]")
		return
	}
	b.WriteByte('[')
	first := true
	for i, k := range m.keys {
		if !m.alive[i] {
			continue
		}
		if !first {
			b.WriteString(", ")
		}
		first = false
		nyShowAny(b, k)
		b.WriteString(": ")
		nyShowAny(b, m.vals[i])
	}
	b.WriteByte(']')
}

// nyCheck is an index that must be in bounds (E0240).
func nyCheck[T any](a *Array[T], i int64, line, col int) int64 {
	if i < 0 || i >= int64(len(a.items)) {
		nyOOB(i, int64(len(a.items)), line, col)
	}
	return i
}

// nyGet is a[i], checked.
func nyGet[T any](a *Array[T], i int64, line, col int) T {
	return a.items[nyCheck(a, i, line, col)]
}

func nyPop[T any](a *Array[T], line, col int) T {
	n := len(a.items)
	if n == 0 {
		nyFail("E0242", "pop() on an empty array", "check `xs.len() > 0` first", line, col)
	}
	v := a.items[n-1]
	a.items = a.items[:n-1]
	return v
}

func nyInsert[T any](a *Array[T], i int64, v T, line, col int) {
	if i < 0 || i > int64(len(a.items)) {
		nyFail("E0240", fmt.Sprintf("index %d is out of bounds for length %d", i, len(a.items)), "insert(i, x) needs 0 <= i <= len", line, col)
	}
	a.items = slices.Insert(a.items, int(i), v)
}

func nyRemove[T any](a *Array[T], i int64, line, col int) T {
	nyCheck(a, i, line, col)
	v := a.items[i]
	a.items = slices.Delete(a.items, int(i), int(i)+1)
	return v
}

func nySwap[T any](a *Array[T], i, j int64, line, col int) {
	nyCheck(a, i, line, col)
	nyCheck(a, j, line, col)
	a.items[i], a.items[j] = a.items[j], a.items[i]
}

func nySlice[T any](a *Array[T], from, to int64, line, col int) *Array[T] {
	nyRange(from, to, int64(len(a.items)), line, col)
	return &Array[T]{items: nyShareAll(slices.Clone(a.items[from:to]))}
}

func nyConcat[T any](a, b *Array[T]) *Array[T] {
	return &Array[T]{items: nyShareAll(slices.Concat(a.items, b.items))}
}

func nyRepeat[T any](a *Array[T], n int64, line, col int) *Array[T] {
	if n < 0 {
		nyFail("E0243", fmt.Sprintf("repeat count must be >= 0, got %d", n), "repeat(n) needs n >= 0", line, col)
	}
	// the longest array every backend makes with repeat
	if len(a.items) > 0 && n > 100000000/int64(len(a.items)) {
		nyOOM(line, col)
	}
	return &Array[T]{items: nyShareAll(slices.Repeat(a.items, int(n)))}
}

// nyExtend is `xs += ys` on a unique `xs` (`xs += xs` doubles it).
func nyExtend[T any](a, b *Array[T]) {
	a.items = append(a.items, nyShareAll(slices.Clone(b.items))...)
}

// nyEqualer is a struct with array fields: it compares them element by element.
type nyEqualer interface{ nyEq(o any) bool }

func (a *Array[T]) nyEq(o any) bool {
	b := o.(*Array[T])
	if len(a.items) != len(b.items) {
		return false
	}
	for i := range a.items {
		if !nyEqual(a.items[i], b.items[i]) {
			return false
		}
	}
	return true
}

// nyEqual is deep equality (no shortcut for the same array: NaN never equals itself).
func nyEqual[T any](a, b T) bool {
	if e, ok := any(a).(nyEqualer); ok {
		return e.nyEq(b)
	}
	return any(a) == any(b)
}

func nyIndexOf[T any](a *Array[T], v T) int64 {
	for i := range a.items {
		if nyEqual(a.items[i], v) {
			return int64(i)
		}
	}
	return -1
}

// nyCmpFloat sorts floats like every backend: NaN after every number, equal values keep their order.
func nyCmpFloat(x, y float64) int {
	lt := func(x, y float64) bool { return x < y || (y != y && x == x) }
	if lt(x, y) {
		return -1
	}
	if lt(y, x) {
		return 1
	}
	return 0
}

// nySortBy is `xs.sort_by(x => key)`: a stable sort of the positions by the keys, then the
// elements move to their places.
func nySortBy[T any, K any](a *Array[T], ks *Array[K], c func(K, K) int) {
	idx := make([]int, len(ks.items))
	for i := range idx {
		idx[i] = i
	}
	slices.SortStableFunc(idx, func(i, j int) int { return c(ks.items[i], ks.items[j]) })
	old := slices.Clone(a.items)
	for k, i := range idx {
		a.items[k] = old[i]
	}
}

// nyCmpOrd orders ints, chars and strings (strings by code points: UTF-8 bytes keep that order).
func nyCmpOrd[K int64 | rune | string](x, y K) int {
	if x < y {
		return -1
	}
	if y < x {
		return 1
	}
	return 0
}

// ---- printing: arrays and structs as Nyra code ----

// nyShower is a value that prints itself (an array or a struct).
type nyShower interface{ nyShowIn(b *strings.Builder) }

func (a *Array[T]) nyShowIn(b *strings.Builder) {
	b.WriteByte('[')
	for i, v := range a.items {
		if i > 0 {
			b.WriteString(", ")
		}
		nyShowAny(b, v)
	}
	b.WriteByte(']')
}

// nyShowAny writes a value as it appears inside an array or a struct (strings and chars quoted).
func nyShowAny(b *strings.Builder, v any) {
	switch x := v.(type) {
	case int64:
		b.WriteString(strconv.FormatInt(x, 10))
	case float64:
		b.WriteString(nyNum(x))
	case bool:
		b.WriteString(strconv.FormatBool(x))
	case rune:
		nyQuoted(b, string(x), '\'')
	case string:
		nyQuoted(b, x, '"')
	case nyShower:
		x.nyShowIn(b)
	}
}

func nyQuoted(b *strings.Builder, text string, quote rune) {
	b.WriteRune(quote)
	for _, c := range text {
		switch c {
		case '\\':
			b.WriteString("\\\\")
		case '\n':
			b.WriteString("\\n")
		case '\t':
			b.WriteString("\\t")
		case '\r':
			b.WriteString("\\r")
		case quote:
			b.WriteRune('\\')
			b.WriteRune(c)
		default:
			b.WriteRune(c)
		}
	}
	b.WriteRune(quote)
}

// nyShow is an array or a struct as print shows it.
func nyShow(v nyShower) string {
	var b strings.Builder
	v.nyShowIn(&b)
	return b.String()
}
