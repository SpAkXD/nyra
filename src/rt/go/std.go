
// ---- standard library: `use input`, `use os`, `use fs`, `use time`, `use random`, `use math`, `use text`

func nyFsFail(what, path, reason string, line, col int) {
	nyFail("E0340", what+" \""+nyShown(path)+"\" ("+reason+")",
		"check the path: it is relative to the folder the program runs in (`fs.exists(path)` tests first)", line, col)
}

func nyIOReason(err error) string {
	switch {
	case os.IsNotExist(err) || strings.Contains(err.Error(), "not a directory"):
		return "not found"
	case os.IsPermission(err):
		return "permission denied"
	case os.IsExist(err):
		return "already exists"
	}
	return "io error"
}

// nyKind: 0 nothing there (also for "" and a path with a NUL), 1 a file, 2 a directory.
func nyKind(path string) int {
	if path == "" || strings.Contains(path, "\x00") {
		return 0
	}
	info, err := os.Stat(path)
	if err != nil {
		return 0
	}
	if info.IsDir() {
		return 2
	}
	return 1
}

// ---- fs ----

func nyStd_fs_read(path string, line, col int) string {
	what := "fs.read: cannot read"
	switch nyKind(path) {
	case 0:
		nyFsFail(what, path, "not found", line, col)
	case 2:
		nyFsFail(what, path, "is a directory", line, col)
	}
	b, err := os.ReadFile(path)
	if err != nil {
		nyFsFail(what, path, nyIOReason(err), line, col)
	}
	if !utf8.Valid(b) {
		nyFsFail(what, path, "not valid UTF-8", line, col)
	}
	return string(b)
}

func nyFsPut(path, text string, appending bool, what string, line, col int) {
	if path == "" || strings.Contains(path, "\x00") {
		nyFsFail(what, path, "not found", line, col)
	}
	if nyKind(path) == 2 {
		nyFsFail(what, path, "is a directory", line, col)
	}
	var err error
	if appending {
		var f *os.File
		f, err = os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
		if err == nil {
			_, err = f.WriteString(text)
			if cerr := f.Close(); err == nil {
				err = cerr
			}
		}
	} else {
		err = os.WriteFile(path, []byte(text), 0o644)
	}
	if err != nil {
		nyFsFail(what, path, nyIOReason(err), line, col)
	}
}

func nyStd_fs_write(path, text string, line, col int) {
	nyFsPut(path, text, false, "fs.write: cannot write", line, col)
}

func nyStd_fs_append(path, text string, line, col int) {
	nyFsPut(path, text, true, "fs.append: cannot append to", line, col)
}

func nyStd_fs_exists(path string, line, col int) bool { return nyKind(path) != 0 }

func nyStd_fs_list(dir string, line, col int) *Array[string] {
	what := "fs.list: cannot list"
	switch nyKind(dir) {
	case 0:
		nyFsFail(what, dir, "not found", line, col)
	case 1:
		nyFsFail(what, dir, "not a directory", line, col)
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		nyFsFail(what, dir, nyIOReason(err), line, col)
	}
	names := make([]string, 0, len(entries))
	for _, e := range entries {
		if !utf8.ValidString(e.Name()) {
			nyFsFail(what, dir, "not valid UTF-8", line, col)
		}
		names = append(names, e.Name())
	}
	slices.Sort(names)
	return &Array[string]{items: names}
}

func nyStd_fs_remove(path string, line, col int) {
	what := "fs.remove: cannot remove"
	kind := nyKind(path)
	if kind == 0 {
		nyFsFail(what, path, "not found", line, col)
	}
	if kind == 2 && len(nyStd_fs_list(path, line, col).items) > 0 {
		nyFsFail(what, path, "not empty", line, col)
	}
	if err := os.Remove(path); err != nil {
		nyFsFail(what, path, nyIOReason(err), line, col)
	}
}

func nyStd_fs_mkdir(path string, line, col int) {
	what := "fs.mkdir: cannot create"
	if path == "" || strings.Contains(path, "\x00") {
		nyFsFail(what, path, "not found", line, col)
	}
	if nyKind(path) != 0 {
		nyFsFail(what, path, "already exists", line, col)
	}
	if err := os.Mkdir(path, 0o777); err != nil {
		nyFsFail(what, path, nyIOReason(err), line, col)
	}
}

// ---- input: standard input as bytes (the same on every system) ----

var nyIn = bufio.NewReaderSize(os.Stdin, 1<<16)

func nyInText(b []byte, what string, line, col int) string {
	if !utf8.Valid(b) {
		nyFail("E0341", what+": the input is not valid UTF-8", "standard input must be UTF-8 text", line, col)
	}
	return string(b)
}

func nyStd_input_line(line, col int) string {
	nyOut.Flush()
	b, _ := nyIn.ReadBytes('\n')
	if n := len(b); n > 0 && b[n-1] == '\n' {
		b = b[:n-1]
		if n := len(b); n > 0 && b[n-1] == '\r' {
			b = b[:n-1]
		}
	}
	return nyInText(b, "input.line", line, col)
}

func nyStd_input_eof(line, col int) bool {
	nyOut.Flush()
	_, err := nyIn.Peek(1)
	return err != nil
}

func nyReadRest() []byte {
	nyOut.Flush()
	b, _ := io.ReadAll(nyIn)
	return b
}

func nyStd_input_all(line, col int) string {
	return nyInText(nyReadRest(), "input.all", line, col)
}

// nyLines: the lines of a text, each without its `\n` (and a `\r` before it); no last empty line.
func nyLines(s string) *Array[string] {
	out := []string{}
	for s != "" {
		l, next := s, ""
		if i := strings.IndexByte(s, '\n'); i >= 0 {
			l, next = strings.TrimSuffix(s[:i], "\r"), s[i+1:]
		}
		out = append(out, l)
		s = next
	}
	return &Array[string]{items: out}
}

func nyStd_input_lines(line, col int) *Array[string] {
	return nyLines(nyInText(nyReadRest(), "input.lines", line, col))
}

// ---- os ----

func nyStd_os_args(line, col int) *Array[string] {
	args := append([]string{}, os.Args[1:]...)
	for _, a := range args {
		if !utf8.ValidString(a) {
			nyFail("E0341", "os.args: an argument is not valid UTF-8", "program arguments must be UTF-8 text", line, col)
		}
	}
	return &Array[string]{items: args}
}

func nyGetenv(name string, line, col int) (string, bool) {
	if name == "" || strings.ContainsAny(name, "=\x00") {
		return "", false
	}
	v, ok := os.LookupEnv(name)
	if ok && !utf8.ValidString(v) {
		nyFail("E0341", "os.env: the value is not valid UTF-8", "environment variables must be UTF-8 text", line, col)
	}
	return v, ok
}

func nyStd_os_env(name string, line, col int) string {
	v, _ := nyGetenv(name, line, col)
	return v
}

func nyStd_os_has_env(name string, line, col int) bool {
	_, ok := nyGetenv(name, line, col)
	return ok
}

func nyStd_os_exit(code int64, line, col int) {
	nyOut.Flush()
	os.Exit(int(code & 255))
}

// ---- time ----

var nyStart = time.Now()

func nyStd_time_now_ms(line, col int) int64 { return time.Now().UnixMilli() }

func nyStd_time_mono_ms(line, col int) float64 {
	return float64(time.Since(nyStart).Nanoseconds()) / 1e6
}

func nyStd_time_sleep_ms(ms int64, line, col int) {
	nyOut.Flush()
	if ms > 0 {
		time.Sleep(time.Duration(ms) * time.Millisecond)
	}
}

// ---- random: the operating system's generator, or after `random.seed(n)` xoshiro128** ----

var nySeeded bool
var nyRS [4]uint32
var nyPool [64]uint32
var nyPoolLeft int

func nyRotl(x uint32, k int) uint32 { return x<<k | x>>(32-k) }

func nyMix32(z uint32) uint32 {
	z = (z ^ z>>16) * 0x85EBCA6B
	z = (z ^ z>>13) * 0xC2B2AE35
	return z ^ z>>16
}

func nyBits32() uint32 {
	if nySeeded {
		s := &nyRS
		r := nyRotl(s[1]*5, 7) * 9
		t := s[1] << 9
		s[2] ^= s[0]
		s[3] ^= s[1]
		s[1] ^= s[2]
		s[0] ^= s[3]
		s[2] ^= t
		s[3] = nyRotl(s[3], 11)
		return r
	}
	if nyPoolLeft == 0 {
		var b [256]byte
		if _, err := rand.Read(b[:]); err != nil {
			nyOut.Flush()
			fmt.Fprintln(os.Stderr, "random: no randomness source available")
			os.Exit(101)
		}
		for i := range nyPool {
			nyPool[i] = uint32(b[4*i]) | uint32(b[4*i+1])<<8 | uint32(b[4*i+2])<<16 | uint32(b[4*i+3])<<24
		}
		nyPoolLeft = 64
	}
	nyPoolLeft--
	return nyPool[nyPoolLeft]
}

func nyStd_random_seed(n int64, line, col int) {
	u := uint64(n)
	half := [2]uint32{uint32(u), uint32(u >> 32)}
	for i := 0; i < 4; i++ {
		nyRS[i] = nyMix32(half[i&1] + uint32(i+1)*0x9E3779B9)
	}
	if nyRS == [4]uint32{} {
		nyRS[0] = 1
	}
	nySeeded = true
}

// nyBits53: 53 random bits; the first draw gives the high 27, the second the low 26.
func nyBits53() int64 {
	a := int64(nyBits32() >> 5)
	b := int64(nyBits32() >> 6)
	return a*67108864 + b
}

func nyStd_random_random(line, col int) float64 { return float64(nyBits53()) / 9007199254740992.0 }

func nyStd_random_range(lo, hi int64, line, col int) int64 {
	n := uint64(hi) - uint64(lo)
	if hi <= lo || n > 9007199254740992 {
		nyFail("E0342", fmt.Sprintf("random.range(%d, %d): need lo < hi and hi - lo <= 2^53", lo, hi),
			"the upper bound is excluded: `random.range(1, 7)` rolls a die", line, col)
	}
	limit := int64(9007199254740992 - 9007199254740992%n)
	r := nyBits53()
	for r >= limit {
		r = nyBits53()
	}
	return int64(uint64(lo) + uint64(r)%n)
}

// ---- math: operations that are exact on every host ----

func nyStd_math_sqrt(x float64, line, col int) float64  { return math.Sqrt(x) }
func nyStd_math_floor(x float64, line, col int) float64 { return math.Floor(x) }
func nyStd_math_ceil(x float64, line, col int) float64  { return math.Ceil(x) }
func nyStd_math_trunc(x float64, line, col int) float64 { return math.Trunc(x) }

// nyStd_math_round: half away from zero (x - trunc(x) is exact).
func nyStd_math_round(x float64, line, col int) float64 {
	t := math.Trunc(x)
	if math.Abs(x-t) >= 0.5 {
		if x < 0 {
			t -= 1
		} else {
			t += 1
		}
	}
	return t
}

// ---- text ----

func nyStd_text_is_int(s string, line, col int) bool {
	if !nyDigits(strings.TrimPrefix(s, "-")) {
		return false
	}
	_, err := strconv.ParseInt(s, 10, 64)
	return err == nil
}

func nyStd_text_is_float(s string, line, col int) bool {
	i, n := 0, len(s)
	digits := func() bool {
		start := i
		for i < n && s[i] >= '0' && s[i] <= '9' {
			i++
		}
		return i > start
	}
	if i < n && s[i] == '-' {
		i++
	}
	if !digits() {
		return false
	}
	if i < n && s[i] == '.' {
		i++
		if !digits() {
			return false
		}
	}
	if i < n && (s[i] == 'e' || s[i] == 'E') {
		i++
		if i < n && (s[i] == '+' || s[i] == '-') {
			i++
		}
		if !digits() {
			return false
		}
	}
	return i == n
}

// nyStd_text_fixed: the exact value of x rounded to d decimals, ties away from zero.
func nyStd_text_fixed(x float64, d int64, line, col int) string {
	if d < 0 || d > 100 {
		nyFail("E0342", fmt.Sprintf("text.fixed: digits must be 0 to 100, got %d", d), "`text.fixed(x, 2)` shows two decimals", line, col)
	}
	if math.IsNaN(x) || math.IsInf(x, 0) {
		return nyNum(x)
	}
	bits := math.Float64bits(x)
	ex := int64(bits >> 52 & 0x7FF)
	m := bits & 0xFFFFFFFFFFFFF
	if ex == 0 {
		ex = 1
	} else {
		m |= 1 << 52
	}
	a := new(big.Int).SetUint64(m)
	a.Mul(a, new(big.Int).Exp(big.NewInt(5), big.NewInt(d), nil))
	s := ex - 1075 + d // x * 10^d = m * 5^d * 2^s
	q := new(big.Int)
	if s >= 0 {
		q.Lsh(a, uint(s))
	} else {
		q.Rsh(a, uint(-s))
		if a.Bit(int(-s-1)) == 1 {
			q.Add(q, big.NewInt(1))
		}
	}
	digits := q.String()
	if d > 0 {
		if len(digits) <= int(d) {
			digits = strings.Repeat("0", int(d)+1-len(digits)) + digits
		}
		digits = digits[:len(digits)-int(d)] + "." + digits[len(digits)-int(d):]
	}
	if bits>>63 == 1 && q.Sign() != 0 {
		return "-" + digits
	}
	return digits
}
