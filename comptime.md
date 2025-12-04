# Wisp Compile-Time Execution (Comptime)

This document describes Wisp's compile-time execution model.

## Overview

Comptime allows code to execute during compilation. This enables:

- Compile-time computation (lookup tables, constants)
- Type reflection and introspection
- Code generation (derive, metaprogramming)
- Build-time IO (loading schemas, configs)

## Syntax

### Comptime Expression

```wisp
let x = comptime some_expression
```

### Comptime Block

```wisp
comptime {
    let data = read_file("config.json")
    let parsed = parse_json(data)
    #insert generate_types(parsed)
}
```

### Intrinsics (Compiler Built-ins)

```wisp
#type_info(T)     // Type reflection
#size_of(T)       // Size in bytes
#align_of(T)      // Alignment
#type_name(T)     // Type name as string
#insert(code)     // Inject generated code
```

Note: `#` is used for intrinsics (compiler operations), `@` is reserved for lifetimes.

### Attributes (Rust-style)

```wisp
#[derive(Clone, Debug)]
struct Point { x: i32, y: i32 }

#[derive(Clone, Serialize(rename_all = "camelCase"))]
struct User { name: String, age: i32 }
```

Attributes use `#[...]` syntax (like Rust) for extensibility and clarity.

## Call-Site Model

Functions are NOT marked as comptime. The **call site** requests comptime evaluation:

```wisp
fn factorial(n: i32) -> i32 {
    if n <= 1 { 1 } else { n * factorial(n - 1) }
}

// Same function, two contexts:
let a = comptime factorial(10)    // Evaluated at compile time
let b = factorial(runtime_value)  // Evaluated at runtime
```

This avoids duplicating functions for comptime vs runtime.

## Comptime-Only Inference

Functions using intrinsics become comptime-only automatically:

```wisp
fn describe(T: Type) -> String {
    #type_info(T).name   // Uses intrinsic
}

// Must be called at comptime:
let name = comptime describe(Point)   // OK
let name = describe(Point)            // Error: requires comptime
```

The compiler infers this - no annotation needed.

## Reflection API

```wisp
comptime {
    let info = #type_info(Point)

    info.name           // "Point"
    info.fields         // [{ name: "x", type: i32 }, ...]
    info.methods        // [{ name: "add", params: [...], ret: ... }]
    info.size           // 8
    info.alignment      // 4
    info.is_struct      // true
    info.is_enum        // false
}
```

## Capabilities

| Capability        | Allowed? |
| ----------------- | -------- |
| Pure computation  | ✅       |
| Memory allocation | ✅       |
| File reads        | ✅       |
| File writes       | ✅       |
| Network           | ✅       |
| Shell commands    | ✅       |

**Full power, no restrictions.** Trust the code you compile.

## LSP Behavior

| Context      | Behavior                                    |
| ------------ | ------------------------------------------- |
| `wisp build` | Full power - all side effects execute       |
| LSP          | Read-only sandbox - writes silently skipped |

Same comptime code, different runtime context. No annotations needed.

```wisp
comptime {
    let data = read_file("config.json")   // Both: works
    write_file("log.txt", "compiled")     // Build: writes. LSP: skipped.
    #insert generate(data)                // Both: generates code
}
```

## Derive Example

### Built-in Derive

```wisp
#[derive(Clone, Debug)]
struct Point { x: i32, y: i32 }
```

### User-Defined Derive

```wisp
// In json_lib.ws
fn derive_json(T: Type) -> Code {
    let info = #type_info(T)
    let mut fields = ""
    for field in info.fields {
        fields += "\"{field.name}\": self.{field.name}.to_json(), "
    }

    return parse_code("
        impl JsonSerialize for {info.name} {
            fn to_json(&self) -> String {
                \"{\" + {fields} + \"}\"
            }
        }
    ")
}

// Usage
#[derive(JsonSerialize)]
struct User { name: String, age: i32 }

user.to_json()  // {"name": "bob", "age": 42}
```

## Compilation Pipeline

```
1. Parse (include comptime blocks, #[derive], #insert)
2. Resolve (first pass)
3. Type check (first pass, including comptime functions)
4. Execute comptime
   - Lower comptime to MIR
   - Interpret MIR
   - #insert → parse generated code → inject into AST
5. Type check (second pass, with generated code)
6. MIR lowering (full program)
7. Codegen
```

## Implementation Components

| Component       | Description                               |
| --------------- | ----------------------------------------- |
| MIR Interpreter | Execute comptime functions                |
| TypeInfo        | Built-in type for reflection              |
| Intrinsics      | `#type_info`, `#size_of`, `#insert`, etc. |
| Code Generation | #insert parses result, injects AST        |
| LSP Sandbox     | Read-only mode for comptime in LSP        |

## Open Questions

- [ ] Exact `TypeInfo` structure and fields
- [ ] Code generation format (strings vs AST nodes vs templates)
- [ ] Error messages for generated code (source mapping)
- [ ] Incremental comptime (caching for fast rebuilds)
- [ ] Comptime debugging story
