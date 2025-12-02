# Wisp Language v1 — Draft Specification and Syntax Reference

Note: This document defines the language core. UI framework concepts (e.g.,
Signal/Effect/Style/Node) are not part of the language. They may appear only
in examples to illustrate syntax usage.

---

## 0. Scope and Goals

- Minimal, orthogonal core with strong typing and safety.
- Expression-oriented: trailing blocks are values; no special "UI mode".
- Ergonomic construction: contextual record/collection literals, named args,
  string interpolation, enum shorthand, numeric suffixes.
- Ownership and borrowing with mostly inferred lifetimes; explicit labels only
  when necessary.
- Deterministic, sandboxed compile-time execution (CTE) and reflection.
- TypeScript-like imports with a single `import` keyword.
- File extension: `.ws`
- Language name: Wisp

---

## 1. Modules, Files, Imports

- Module path = file path (TypeScript-like).

  - `src/net/http.ws` → module `net.http`
  - `src/net/http/index.ws` → module `net.http`
  - Error if both `http.ws` and `http/index.ws` exist.

- Imports (single keyword: `import`):

  - Module namespace:
    ```
    import net/http
    let c = net.http.Client.new()
    ```
  - Named imports:
    ```
    import net/http { Client, Request }
    let c = Client.new()
    ```
  - Aliases:
    ```
    import net/http as http
    import net/http { Client as HttpClient }
    ```
  - Relative:
    ```
    import ./util
    import ./util { now }
    ```

- Visibility:

  - Private by default.
  - `pub` for public; `pub(crate)` for package-internal.
  - Re-exports:
    ```
    pub import net/http { Client, Request }
    pub import net as networking
    ```

- No side effects at import:
  - Top-level code must be const-evaluable. No runtime effects on import.

---

## 2. Declarations and Blocks

- Program is a sequence of global items:

  - Type definitions (struct/enum/alias/trait).
  - Function definitions.
  - Const definitions.
  - (Top-level `let` discouraged; allowed if const-evaluable.)

- Blocks:

```
{
    // statements
}
```

- Statement forms:
  - `let` declaration
  - `const` declaration
  - Assignment
  - Expression statement (semicolon optional in contexts described below)

---

## 3. Functions and Calls

- Function definition:

```
fn name(params) -> ReturnType {
    // body
}
```

- Parameters:
  - Immutable by default; use `mut` for mutable bindings.
  - Lifetime labels via `@label` suffix.

```
fn add(a: i32, b: i32) -> i32 { a + b }
fn increment(mut x: i32) -> i32 { x += 1; x }  // x is mutable locally
```

- Named arguments at call sites (global feature):

```
open(path: "/tmp/x", mode: ReadWrite)
```

- Trailing block binds to last parameter (core rule):
  - If the last param is `T`: the block is a statement block whose final
    expression (no trailing semicolon) must evaluate to `T`.
  - If the last param is a collection type (e.g., `Vec<T>` or anything that
    implements `FromIterator<T>`): the block body is a statement block; each
    trailing expression is collected as an element via `FromIterator`.
  - If the last param is a record/slotted type: the block must evaluate to
    that record (see contextual records).

Examples:

```
fn Text(style: Style = [], text: String) -> Node
Text { "Hello" }  // block → last param `text`

fn Row(style: Style = [], children: Vec<Node>) -> Node

Row(style: [Row, Gap.3]) {
    Text { "A" }
    Text { "B" }
}
```

- Lambdas (closures):

```
(args) -> expr
(args) -> { block }
```

- Local functions allowed inside other functions:

```
fn outer() {
    fn inner(x: i32) -> i32 { x + 1 }
    inner(2)
}
```

- Auto-deref in assignment contexts for `&mut` params in closures:

```
counter.update((v) -> v += 1)  // v: &mut i32; `v += 1` auto-deref
```

---

## 4. Expressions and Literals

- Everything is an expression by default.
- Supported literals:

  - Integer, float
  - Char, string
  - Interpolated string: `"Hello {expr}"` (use `{{` `}}` to escape braces)
  - Tuple: `(a, b, c)`
  - Contextual record literal: `{ field: value, ... }` (requires expected type)
  - List/collection literal: `[ ... ]` (type-directed)
  - Map literal: `{ "k": v, ... }` (type-directed; requires expected map type)

- Control flow expressions:

  - `if cond { ... } else { ... }` (expression form)
  - `match expr { ... }` (exhaustive by default)
  - Loops:
    - `for pat in expr { ... }`
    - `while cond { ... }`

- Operators:
  - Standard arithmetic, logical, comparison, bitwise (precedence table TBD).
  - Assignment forms: `=`, `+=`, `-=`, etc.

---

## 5. Records (Structs) and Contextual Record Literals

- Struct definition:

```
struct Point { x: i32, y: i32 }
```

- Contextual record literal (type-directed):

```
let p: Point = { x: 1, y: 2 }
```

- Closed by default: unknown fields are errors.
- Spread/update:
  ```
  let base: Point = { x: 1, y: 2 }
  let q: Point = { y: 5, ..base }  // last-wins per field
  ```
- Defaults:

  - `@defaults` on a type enables partials filled by `Type::defaults()`.

  ```
  @defaults
  struct Server { host: String, port: u16 }

  impl Server {
      fn defaults() -> Server { { host: "0.0.0.0", port: 8080 } }
  }

  let s: Server = { port: 9090 }  // host filled from defaults
  ```

---

## 6. Collections and Maps

- List/collection literal `[ ... ]` constructs the expected collection when it
  implements `FromIterator<T>`.

```
let ids: Vec<i32> = [1, 2, 3]
let flags: Set<Perm> = [Read, Write]
```

- Conditionals and loops inside:
  ```
  let feats: Set<Feat> = [
      Core,
      if simd { Simd },
      for f in extra { f },
  ]
  ```
- Spread:
  ```
  let base = [1, 2]
  let xs: Vec<i32> = [..base, 3, 4]
  ```
- Empty `[]` requires type context.

- Map literal `{"k": v, ...}` constructs the expected map type when it
  implements `FromIterator<(K,V)>`.

```
let headers: Map<String, String> = {
    "Accept": "json",
    "User-Agent": "X",
}
```

- Conditionals, loops, spread allowed:
  ```
  let extra = { "gzip": "true" }
  let cfg: Map<String, String> = {
      "h2": "true",
      if debug { "log": "debug" },
      ..extra
  }
  ```
- Disambiguation rules for `{ ... }`:
  - If the expected type is a map (`Map<K, V>` or similar), it's a map literal.
  - If the expected type is a record/struct, it's a contextual record literal.
  - Otherwise, it's parsed as a block expression.
  - When ambiguous (no type context), you must provide an explicit type annotation:
    ```
    let m: Map<String, String> = { "key": "value" }  // map
    let p: Point = { x: 1, y: 2 }                     // record
    let x = { 42 }                                    // block returning 42
    let m = { "key": "value" }                        // ERROR: ambiguous, needs type
    ```

---

## 7. Enums and Pattern Matching

- Enum definition:

```
enum Mode { ReadOnly, ReadWrite }

enum Message {
    Ping,
    Data(bytes: Bytes),
    Error(code: i32, text: String),
}
```

- Contextual variant shorthand:

  - If expected type is known, `ReadWrite` is allowed; otherwise use `Mode.ReadWrite`.

- Numeric payload support:

  - Canonical: `Gap(3)`, `Radius(12)`, `Color(Neutral, 500)`
  - Dot-numeric sugar in variant-expected contexts:
    - `Gap.3` ≡ `Gap(3)`
    - `Radius.12` ≡ `Radius(12)`
    - `Color.Neutral.500` ≡ `Color(Neutral, 500)`
  - If ambiguous, require qualification.

- Match (exhaustive by default):

```
match msg {
    Ping -> handle_ping(),
    Data { bytes } -> use_bytes(bytes),
    Error(code, text) -> log_err(code, text),
}
```

- `@non_exhaustive` enums require a `_` arm.

- Variant payloads can be named or positional at call sites.

---

## 8. Traits and Implementations

- Trait definition:

```
trait Display {
    fn fmt(&self) -> String
}

trait Iterator<T> {
    fn next(&mut self) -> Option<T>
}
```

- Trait with default implementations:

```
trait Greet {
    fn name(&self) -> &str
    fn greet(&self) -> String {
        "Hello, {self.name()}!"
    }
}
```

- Implementing traits for types:

```
impl Display for Point {
    fn fmt(&self) -> String {
        "({self.x}, {self.y})"
    }
}
```

- Inherent implementations (methods on types without traits):

```
impl Point {
    fn new(x: i32, y: i32) -> Point {
        { x, y }
    }

    fn distance(&self, other: &Point) -> f64 {
        let dx = self.x - other.x
        let dy = self.y - other.y
        ((dx * dx + dy * dy) as f64).sqrt()
    }
}
```

- Trait bounds on generics:

```
fn print_all<T: Display>(items: &[T]) {
    for item in items {
        print(item.fmt())
    }
}

// Multiple bounds
fn process<T: Clone + Display>(item: T) -> T {
    print(item.fmt())
    item.clone()
}

// Where clause for complex bounds
fn merge<K, V>(a: Map<K, V>, b: Map<K, V>) -> Map<K, V>
where
    K: Eq + Hash,
    V: Clone,
{
    // ...
}
```

- Built-in traits (auto-derived or implemented by compiler):
  - `Copy`: bitwise copy semantics (primitives, simple structs)
  - `Clone`: explicit `.clone()` for deep copies
  - `Send`: safe to transfer between threads
  - `Sync`: safe to share references between threads
  - `FromIterator<T>`: construct from iterator (enables `[]` literals)

---

## 9. String Interpolation

- `"Hello {expr}"` where `expr` is any expression.
- Escapes for literal braces: `{{` and `}}`.
- Formatting specifiers can be added in a future release.

---

## 10. Ownership, Borrowing, and Lifetimes

- Moves by default; borrows explicit:

  - `&T` shared, `&mut T` unique.
  - Flow-based lifetime inference; explicit annotations appear only when needed.

- Lifetime labels (function-local, simple):
  - Label a parameter's borrow source: `param@a: &T`
  - Tie an output to a source: `&@a U`

Examples:

```
fn tail(xs@x: &[u8]) -> &@x [u8] { &xs[1..] }

fn pick_either(a@x: &str, b@x: &str, take_a: bool) -> &@x str {
    if take_a { a } else { b }
}
```

- Methods elide lifetimes in common cases:

```
impl Buffer {
    fn bytes(&self) -> &[u8] { &self.data }
}
```

- Struct fields may carry labels for references:

```
struct View { text@t: &str, head@t: &str }
impl View { fn head(&self) -> &@t str { self.head } }
```

---

## 11. Errors and Defer

- Core types: `Result<T, E>` and `Option<T>` with `?` operator.

```
fn load(path: &str) -> Result<String, IoError> {
    let bytes = read_file(path)?
    parse_utf8(bytes)
}

fn first<T>(items: &[T]) -> Option<&T> {
    if items.is_empty() { None } else { Some(&items[0]) }
}
```

- `?` propagates `Err` or `None` early; works in functions returning `Result` or `Option`.

- Defer:

```
defer { cleanup() }  // runs at scope exit
```

- Panic and assertions available; release behavior configurable.

---

## 12. Concurrency (Library-Level APIs)

Note: Not part of core language grammar; shown here to clarify intended usage.

- Tasks/threads:

```
let h = spawn(() -> work())
join(h)?
spawn_detached(() -> bg())
```

- Channels/timers:

```
let (tx, rx) = channel<String>()
tx.send("ready")
for msg in rx { log(msg) }

sleep(150ms)
for t in interval(1s) { tick() }
```

- UI thread pattern:

```
ui_enqueue(() -> model.set(data))
```

- Types derive `Send`/`Sync` rules; UI-bound types are `!Send`.

---

## 13. Compile-Time Execution (CTE) and Reflection

- `const fn` are pure and run at compile time when inputs are const.

```
const FIB10: u64 = fib(10)
const fn fib(n: u64) -> u64 { if n < 2 { n } else { fib(n-1) + fib(n-2) } }
```

- `@comptime` functions may do sandboxed IO/codegen during build.

```
@comptime
fn load_text(path: &str) -> String { read_text(path) }

const SCHEMA: &str = load_text("schema.idl")
```

- Reflection available at compile time only:

```
@comptime
fn fields_of<T>() -> &[FieldInfo] { /* ... */ }
```

- Generators may return a `Module` or values; generated modules are importable:

```
@comptime
fn gen_routes() -> Module { /* ... */ }

import gen_routes() as routes
```

- Sandbox:
  - Default: no network; reads limited to project root unless configured.
  - Results cached by input hash.

---

## 14. Units and Implicit Conversions

- Numeric suffixes (units) via std/user `@suffix` const fns (type-directed):

```
// in std or user type
impl Duration {
    @suffix("ms")
    const fn from_ms(x: u64) -> Duration { Duration(x) }

    @suffix("s")
    const fn from_s(x: u64) -> Duration { Duration(x * 1000) }
}

let t: Duration = 250ms
```

- Implicit single-field wrappers (newtype coercions) via `@implicit(Inner)`:

```
@implicit(u64)
struct UserId(u64)

fn set_user(id: UserId)
set_user(42)  // wraps implicitly
```

- Empty literals require type context:
  - `[]` needs a target collection type.
  - `{}` needs a target map type (otherwise it's an empty block).

---

## 15. Pattern Matching and Destructuring

- Tuple destructuring:

```
let p: (i32, i32) = (5, 2)
let (x, y) = p
```

- Record/struct destructuring:

```
struct Point { x: i32, y: i32, z: i32 }
let p = Point { x: 3, y: 5, z: 7 }

// Partial destructuring — match field names
let (x, z) = p              // x: 3, z: 7

// Rename during destructuring with `->`
let (y -> yPos, z -> zPos) = p   // yPos: 5, zPos: 7

// Ignore remaining fields
let (x, ..) = p             // x: 3
```

- Enum destructuring in `match`:

```
match msg {
    Data(bytes) -> use_bytes(bytes),
    Error(code, text) -> log_err(code, text),
    _ -> {}
}
```

---

## 16. Formatting and Lints (Non-Normative)

- Preferred casing:

  - Types, enum variants: PascalCase.
  - Functions, variables, fields: snake_case.
  - Enums with numeric families: use canonical `Family(number)`; allow dot-numeric sugar in variant contexts (`Family.12`, `Color.Neutral.500`).

- Imports grouped and sorted (std, external, internal).

- Lints:
  - Warn on unknown named argument.
  - Warn on last-wins override (option to silence with `@override`).
  - Warn on unused imports/variables (allow `_name` to silence).
  - Warn on ambiguous enum variant; suggest qualification.

---

## 17. Desugaring Notes (Appendix)

- Trailing block → last parameter:

```
// Source:
Row(style: [Row, Gap.3]) {
    Text { "A" }
    Text { "B" }
}

// Conceptual desugar:
Row([Row, Gap.3], vec![
    Text([], "A"),
    Text([], "B"),
])
```

- Contextual record literal:

```
// Source (with type context `Server`):
{ port: 9090, ..base }

// Desugar:
Server { port: 9090, ..base }
```

- List literal with spreads/if/for:

```
[ ..xs, if cond { y }, for z in zs { z } ]
// Desugar to iterator chaining and FromIterator
```

- Enum dot-numeric sugar:

```
Gap.3                 // -> Gap(3)
Color.Neutral.500     // -> Color(Neutral, 500)
```

---

## 18. Grammar Sketch (EBNF-like)

Note: High-level; detailed operator precedence to be finalized.

```
Program         := GlobalItem*
GlobalItem      := ImportDecl | PubDecl | TypeDef | FuncDef | ConstDef | ImplBlock

// --- Imports ---
ImportDecl      := 'import' ImportPath (ImportAlias? | ImportNamed | ImportAlias ImportNamed) ';'?
ImportPath      := ('..' '/')* (Ident ('/' Ident)*) | ModulePath
ImportAlias     := 'as' Ident
ImportNamed     := '{' ImportSpec (',' ImportSpec)* ','? '}'
ImportSpec      := Ident ('as' Ident)?

PubDecl         := 'pub' (ImportDecl | TypeDef | FuncDef | ConstDef | ImplBlock)

// --- Type Definitions ---
TypeDef         := StructDef | EnumDef | TraitDef | AliasDef
AliasDef        := 'type' Ident GenericParams? '=' TypeExpr

StructDef       := 'struct' Ident GenericParams? '{' StructFieldList? '}'
StructFieldList := StructField (',' StructField)* ','?
StructField     := Ident ('@' Ident)? ':' TypeExpr

EnumDef         := 'enum' Ident GenericParams? '{' EnumMember* '}'
EnumMember      := Ident MemberPayload? ','?
MemberPayload   := '(' ParamTypeList? ')'

// --- Traits ---
TraitDef        := 'trait' Ident GenericParams? TraitBounds? '{' TraitItem* '}'
TraitBounds     := ':' TypeExpr ('+' TypeExpr)*
TraitItem       := FuncSig (Block | ';')

// --- Implementations ---
ImplBlock       := 'impl' GenericParams? ImplTarget '{' ImplItem* '}'
ImplTarget      := TypeExpr                           // inherent impl
                 | TypeExpr 'for' TypeExpr            // trait impl
ImplItem        := FuncDef

// --- Generics ---
GenericParams   := '<' GenericParam (',' GenericParam)* ','? '>'
GenericParam    := Ident (':' TypeBounds)?
TypeBounds      := TypeExpr ('+' TypeExpr)*

WhereClause     := 'where' WherePredicate (',' WherePredicate)* ','?
WherePredicate  := TypeExpr ':' TypeBounds

// --- Functions ---
FuncDef         := FuncSig WhereClause? Block
FuncSig         := 'fn' Ident GenericParams? '(' ParamList? ')' ('->' TypeExpr)?
ParamList       := Param (',' Param)* ','?
Param           := 'mut'? Ident ':' TypeExpr ('@' Ident)?

ConstDef        := 'const' Pattern (':' TypeExpr)? '=' Expr

// --- Type Expressions ---
TypeExpr        := Ident GenericArgs?
                 | '&' 'mut'? TypeExpr
                 | '&' '@' Ident TypeExpr
                 | '[' TypeExpr ']'
                 | '[' TypeExpr ';' Expr ']'
                 | '(' TypeExpr (',' TypeExpr)* ')'
                 | 'fn' '(' TypeList? ')' ('->' TypeExpr)?
GenericArgs     := '<' TypeExpr (',' TypeExpr)* ','? '>'
TypeList        := TypeExpr (',' TypeExpr)* ','?

// --- Statements ---
Block           := '{' Stmt* '}'
Stmt            := LetStmt ';'
                 | ConstDef ';'
                 | AssignStmt ';'
                 | DeferStmt
                 | ExprStmt ';'?
LetStmt         := 'let' 'mut'? Pattern (':' TypeExpr)? ('=' Expr)?
AssignStmt      := LValue AssignOp Expr
AssignOp        := '=' | '+=' | '-=' | '*=' | '/=' | '%=' | '&=' | '|=' | '^='
DeferStmt       := 'defer' Block
ExprStmt        := Expr

// --- Expressions ---
Expr            := Literal
                 | Ident
                 | PathExpr
                 | TupleLit
                 | RecordLit
                 | ListLit
                 | MapLit
                 | CallExpr
                 | MethodExpr
                 | LambdaExpr
                 | IfExpr
                 | WhileExpr
                 | ForExpr
                 | MatchExpr
                 | UnaryExpr
                 | BinaryExpr
                 | StringInterp

CallExpr        := Expr '(' ArgList? ')' TrailingBlock?
MethodExpr      := Expr '.' Ident GenericArgs? '(' ArgList? ')'
ArgList         := Arg (',' Arg)* ','?
Arg             := (Ident ':')? Expr
TrailingBlock   := Block

LambdaExpr      := '(' ParamList? ')' '->' (Expr | Block)

IfExpr          := 'if' Expr Block ('else' (Block | IfExpr))?
WhileExpr       := 'while' Expr Block
ForExpr         := 'for' Pattern 'in' Expr Block

MatchExpr       := 'match' Expr '{' MatchArm* '}'
MatchArm        := Pattern '->' (Expr | Block) ','?

// --- Patterns ---
Pattern         := '_'
                 | Ident
                 | Literal
                 | TuplePattern
                 | RecordPattern
                 | EnumPattern
TuplePattern    := '(' Pattern (',' Pattern)* ','? ')'
RecordPattern   := '(' RecordPatternField (',' RecordPatternField)* ','? '..'? ')'
RecordPatternField := Ident ('->' Ident)?
EnumPattern     := Ident ('(' Pattern (',' Pattern)* ')')?

// --- Literals ---
TupleLit        := '(' Expr (',' Expr)+ ')'
RecordLit       := '{' RecordItems? '}'
RecordItems     := (RecordItem (',' RecordItem)* (',' '..' Expr)?) | ('..' Expr)
RecordItem      := Ident ':' Expr

ListLit         := '[' ListItems? ']'
ListItems       := Elem (',' Elem)* ','?
Elem            := Expr | IfExpr | ForExpr | Spread
Spread          := '..' Expr

MapLit          := '{' MapItems? '}'
MapItems        := MapItem (',' MapItem)* (',' '..' Expr)? | '..' Expr
MapItem         := Expr ':' Expr | IfExpr | ForExpr

StringInterp    := '"' (TextSegment | '{' Expr '}')* '"'
```

Tokens:

- `Ident`: `[A-Za-z_][A-Za-z0-9_]*`
- `ModulePath` (for qualified references in code): `Ident ('.' Ident)*`
- Numeric literals support suffixes resolved by expected type via `@suffix`.

---

## 19. Non-Goals (v1)

- Macros: not needed; use CTE + reflection.
- Runtime reflection: compile-time only.
- Complex async/await runtime: tasks + channels suffice for v1.
- C-style `for(;;)` loops: may consider later; `for-in` and `while` first.

---

## 20. Appendix: Design Rationale (Brief)

- Trailing block → last parameter: general, domain-agnostic; enables pipelines,
  queries, and UI-like builders without special syntax.
- Contextual literals: reduce repetition; stay type-safe and toolable.
- Enum shorthand + dot-numeric: compact tokens without losing typing.
- Lifetime labels: local and minimal; avoid angle-bracket ceremony in 90% cases.
- CTE: replaces macros; deterministic, cached; reflection at compile time only.
- Imports: TS-like for clarity; single keyword; no mod declarations.
