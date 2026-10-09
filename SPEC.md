# s

A postfix scripting language. Words accumulate on a stack; the word that acts stands at the right and consumes what stands to its left. Operators on names are suffixes: the last character of a word decides what the word does.

Reference implementation: `src/rt.rs` (semantics, interpreter, runtime), `src/compile.rs` (s → Rust). Tests: `tests/cases/*.s` with expected `*.out`, run through both backends by `cargo test --release`.

## 1. Usage

```
s [file.s] [args...]                  interpret (default ./main.s)
s --src file.s [args...]              interpret
s run file.s [args...]                interpret
s rs file.s [-o out.rs] [-e file]...  emit one self-contained Rust file
s build file.s [-o bin] [-e file]...  emit and compile with rustc -O (--keep keeps the .rs)
```

The emitted `.rs` depends only on `std` and builds with `rustc -O --edition 2021 out.rs`.

## 2. Units and tokens

A unit is a source string run as one sequence of operations: a file, a script body, a matched table value, a thread message.

Whitespace is space, newline, tab, carriage return. A unit is scanned left to right into tokens.

- **Brace literal** `{...}`. Braces nest; the literal ends at the matching `}`. At depth 1, `~name~` interpolates the value of `name` (§4.3) when the literal is evaluated; `name` is non-empty and contains no whitespace, `{`, `}` or `~`. `~~` yields `~`. A `~` that does not open a well-formed interpolation is literal. Text at depth ≥ 2 is raw, including `~`.
- **Quote literal** `'...'`. Raw text up to the next `'`. No interpolation, no brace meaning.
- **Word**. A maximal run of non-whitespace characters containing no `{` or `'`.

Characters immediately before a `{` or `'` (no whitespace between) are a prefix of that literal: `pre{fix}` is the literal `prefix`. Characters immediately after a closing `}` or `'` begin a new token.

An unclosed brace or quote is a parse error for the unit.

## 3. Values and state

A value is text. The runtime may hold a value as a number; a number's text is its canonical form: shortest round-trip decimal, no exponent, no trailing `.0`, `-0` written `0`, `NaN`, `inf`, `-inf`. Reading text as a number trims whitespace and parses a decimal float; unparsable text reads as `0`. Representation is unobservable: every operation behaves as if all values were text in canonical form.

State of one interpreter:

| name | content |
|---|---|
| stack | sequence of values |
| ow | boolean, initially false |
| mem | name → value |
| numbers | name → number |
| scripts | name → unit |
| lists | name → sequence of values |
| replacements | word text → value |
| files | name → open file handle with eof flag |
| threads | index → worker |

`mem`, `numbers`, `scripts` and `lists` are separate namespaces over the same names.

Reserved names used by commands: `=` arithmetic default destination, `n` repeat counter, `r` / `rc` shell output and exit code, `rn` prng default destination, `$` list pop result, `err` error text, `eof` end-of-file flag, `out` thread result, `threads` last spawned index, `_` stdin default destination, `args` / `argc` command line.

At start, list `args` holds the command-line arguments after the program file and `argc` their count.

## 4. Words

A word is classified in this order:

1. If the whole word is a command name (§5), it is that command.
2. Else, if the word has at least two characters and its last character is one of `? ! ~ $ ` # ; : .`, it is a suffix form over the stem (the word minus its last character).
3. Else it is a literal word.

A command name is therefore never a literal word: `w`, `r`, `n`, `l`, `f`, `t`, `mv` and the symbol commands must be written as `{w}` or `'w'` when meant as data.

### 4.1 Literals

A literal word pushes its text, unless `replacements` holds the text, in which case it pushes the replacement value. Brace and quote literals push their text and are never replaced.

### 4.2 Suffix forms

| form | effect |
|---|---|
| `x?` | if ow is true, act as word `x` (classified again); else nothing |
| `x!` | if ow is false, act as word `x`; else nothing |
| `x~` | lookup (§4.3), then dispatch the value: if its text is a command name, run that command; else push it as a literal word (§4.1) |
| `` x` `` | mem[x] = the stack joined with single spaces (a single value is stored as is); clear stack |
| `x#` | numbers[x] = first stack value as number (0 if empty); clear stack |
| `x;` | numbers[x] += first stack value (1 if empty; unset counts as 0); clear stack |
| `x:` | numbers[x] −= first stack value (1 if empty); clear stack |
| `x$` | scripts[x] = the stack joined with single spaces, with every `\|` replaced by `~`; clear stack |
| `x.` | call x (§6) |
| `x..` | call x, never as a repeat (§6) |

`?` and `!` compose with every other form: `>?`, `x~!`, `loop.?`, `<-?`.

### 4.3 Lookup

The value of `x` is mem[x] if set, else numbers[x] if set, else a fresh random unsigned 64-bit integer if `x` is `?`, else `404`.

`x~` dispatches the value; `{~x~}` pushes it as data. When a value can be a command name, push it with braces.

## 5. Commands

Every command reads its arguments from the stack, A0 being the bottom. Unless stated, a command clears the stack when done. Fewer arguments than required is an error. Square brackets mark optional arguments. `dest` defaults are shown after `=`.

### Output, input, shell

| word | args | effect |
|---|---|---|
| `>` | any | write newline, then each value preceded by one space |
| `>_` | any | write each value, no separators, no newline |
| `<_` | [dest=`_`] | read one line of stdin without its line ending into mem[dest]; mem[eof] = 1 at end of input, else 0 |
| `<` | cmd | run `sh -c cmd` (`cmd /C` on Windows); mem[r] = stdout, numbers[rc] = exit code; stderr passes through |

### Files by path

| word | args | effect |
|---|---|---|
| `w` | path text | write file |
| `+w` | path text | append to file, creating it |
| `mv` | from to | move, overwriting |
| `r` | path | mem[path] = file contents |
| `r@` | path off len dest | mem[dest] = up to len bytes from byte off |

### File handles

Modes: `r w a r+ w+ a+`, `b` ignored. Handles are named.

| word | args | effect |
|---|---|---|
| `f` | path name mode [errcode] | open |
| `+f` | name off key [errcode] | write mem[key] at byte off |
| `@f` | name off len dest | mem[dest] = up to len bytes at off; eof flag = fewer than len read |
| `f_` | name text [errcode] | write text and newline at the current position |
| `_f` | name dest | mem[dest] = next line without its line ending; eof flag = no line was read |
| `?eof` | name | mem[eof] = 1 if the handle's eof flag is set or no such handle, else 0 |
| `f-` | name | close |

With `errcode` given, a failure sets mem[err] to the message, clears the stack and runs `errcode` as a unit; the stack is then left as that unit leaves it. Without it, failure is an error.

### Numbers

| word | args | effect |
|---|---|---|
| `+ - * / ^` | a b [dest=`=`] | numbers[dest] = a op b; `^` is power |
| `%` | a b [dest=`=`] | numbers[dest] = Euclidean remainder, sign of the result ≥ 0 for b > 0 |
| `n` | value name | numbers[name] = value |
| `#prng` | [seed [seed2 [dest=`rn`]]] | with a seed, reseed the generator from (seed, seed2=0); numbers[dest] = next float in [0, 1) |
| `` `prng `` | [seed [seed2 [dest=`rn`]]] | as `#prng`; mem[dest] = 32 letters from `a-zA-Z` |

The generator is xoshiro256** seeded through splitmix64; the same seeds give the same sequence on every platform. Without a seed, the generator continues from its state, which starts from operating-system randomness.

### Conditions

| word | args | ow becomes |
|---|---|---|
| `=` | a b | a and b have the same text |
| `!=` | a b | not the same text |
| `&` | a b | ow and same text |
| `;` | a b | ow and not the same text |
| `==` | a b | a = b as numbers |
| `<<` `>>` `<<=` `>>=` | a b | a <, >, ≤, ≥ b as numbers |
| `!!` | | not ow |

### Lists

Indices count from 0; a negative index counts from the end.

| word | args | effect |
|---|---|---|
| `l` | value list | append |
| `-l` | list | mem[`$`] = removed last value, `404` if empty |
| `[l]` | list i dest | mem[dest] = item i; nothing if out of range |
| `l=` | value list i | item i = value; nothing if out of range |
| `l-` | list | delete list |
| `#l` | list [dest=`=`] | numbers[dest] = length (0 if no list) |
| `/l` | text list [sep] | list = parts of text: split on whitespace runs without sep, into characters when sep is empty, else on sep |
| `l/` | list dest [sep=` `] | mem[dest] = items joined with sep |
| `#s` | text [dest=`=`] | numbers[dest] = number of characters |

### Code

| word | args | effect |
|---|---|---|
| `?)` | table key [default] | table is read as whitespace-separated pairs `k v`; the first k equal to key gives v, else default; the stack is cleared and that text runs as a unit, leaving the stack as the unit leaves it |
| `&&&` | count name | for n = 0 … count−1: clear stack, numbers[n] = n, call name |
| `><` | value word | replacements[word] = value |
| `//` | any | clear stack |
| `<-` | | end the current unit; the stack is kept |
| `<--` | [code=0] | flush output and end the process with code |

`//` clears what stands to its left, so a comment is written before it: `this is a note //`. A brace literal makes a comment that may contain command words: `{any > text} //`.

### Threads

| word | args | effect |
|---|---|---|
| `t` | path... | per path, start a worker; numbers[threads] = its index |
| `<t` | code i | send code to worker i |
| `>t` | i | wait for worker i's next result, clear stack, run it as a unit, leaving the stack as the unit leaves it |
| `-t` | i | stop worker i; its index is not reused |
| `***` | path... | run each file in its own fresh interpreter in parallel; when all finish, run each one's mem[out] in order, as units |

A worker is a fresh interpreter that runs its file, then repeatedly takes a message, runs it as a unit, and if mem[out] is set, sends it back and unsets it. The message `__stop__` stops it. A bad index sets mem[err] to `Invalid thread index`; a closed worker sets `thread closed`. Workers share nothing with the parent except messages.

## 6. Scripts and calls

`body name$` defines a script. `|` in the body becomes `~` at definition, so `x|` inside a body is the deferred lookup `x~`.

`name.` calls:

- With exactly one value on the stack, that value is a repeat count, a whole number ≥ 0: the stack is cleared, and the script runs count times, each run starting from an empty stack; the stack is cleared afterwards.
- Otherwise the script runs once on the current stack. Values left by the caller are visible to it, and values it leaves remain for the caller.

`name..` always runs once on the current stack.

If no script `name` exists, a file at path `name` is run as the unit; if neither exists, it is an error.

A call that is the last operation of a unit is a tail call: it replaces the current unit instead of nesting in it, so recursion in tail position runs in constant space. `loop.?` as the last word of `loop` is a loop.

`<-` ends only the innermost running unit.

## 7. Errors

An error writes `s: message` to stderr, flushes stdout and ends the process with code 1. Errors: missing arguments, a bad repeat count, an unparsable unit, a missing script or file, file and shell failures without an error code unit.

## 8. Compilation

`s rs` translates a program into Rust with the same behaviour as the interpreter.

- The runtime (`rt.rs`) is copied verbatim into the output; it contains the interpreter, which runs any unit not known at compile time.
- Every statically known unit becomes a Rust function `fn cN(rt: &mut Rt) -> Flow` (a chunk): the main file, every `-e` file, and every script body whose text is fully known at its `$` site. Script bodies nested in script bodies are found by the same rule.
- Names are interned to integers at compile time; the runtime interns the same table first, so names in chunks are array indices.
- A `$` site carries the chunk compiled for the body it expects. At run time the body text is compared with the chunk's source; on a match the script is the chunk, otherwise it is looked up among all chunks by text, otherwise parsed and interpreted. The same lookup applies to any unit run from text (`?)`, `>t`, files). Compiled and interpreted units call each other freely.
- Tail calls compile to `return Flow::Tail(name)`; a trampoline runs them.
- A run of up to three literal or lookup words followed by an arithmetic, comparison, or number-setting operation compiles to a guarded fast path computing directly on numbers. The guard holds when the stack is empty, there are no replacements, and no looked-up value is a command name; otherwise the plain operation sequence runs.
- `-e path` compiles a file into the program; `t`, `***` and `name.` use the compiled file for that exact path instead of reading it.
- The program runs on a thread with a 256 MiB stack; workers get 64 MiB.

## 9. Changes from s.v

- Exact command names are matched before suffixes; `!!` works, lone `;` is the command, lone `:` is a literal.
- `x;` increments and `x:` decrements, as the `Chunk` enum states; both default to 1 and clear the stack.
- A script call no longer pushes an empty value afterwards.
- `?` and `!` re-classify their stem, so suffixes compose.
- Braces nest; interpolation names exclude whitespace, so bodies like `{x~ y~ >}` keep their lookups for run time without `|`.
- Quotes are raw; `{` inside quotes is text.
- Units are parsed independently; an open brace in one unit no longer captures text of the caller.
- `?)` handles a last pair without trailing whitespace, runs the default only on a miss, clears the stack before running the code and leaves what that code leaves.
- `&&&` sets `n` before each run.
- Repeat count 0 runs zero times; a non-numeric single argument to `name.` is an error instead of a panic.
- `-t` keeps indices stable and no longer blocks.
- Numbers print canonically (`3`, not `3.0`).
- Tab and carriage return are whitespace.
- Added: `>_ <_ % & == << >> <<= >>= l= #l /l l/ #s _f <- <--`, `x..`, tail calls, `args`/`argc`, numbers[rc].

## 10. Web editor

`web/srt.js` is the browser runtime, a port of `src/rt.rs`; `node web/test.js` runs it against `tests/cases`. `node web/build.js` writes `web/s-editor.html` with the runtime and examples inlined. In the browser, files live in the editor tabs, `<` runs a built-in shell (echo, cat, ls, rm, cp, mv, pwd, true, false, exit) over them, stdin comes from the stdin box, and workers run in step with the program, so `>t` with no pending result is an error instead of a wait.
