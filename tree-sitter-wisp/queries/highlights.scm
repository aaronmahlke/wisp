; Keywords
[
  "fn"
  "let"
  "mut"
  "if"
  "else"
  "while"
  "loop"
  "return"
  "break"
  "continue"
  "struct"
  "enum"
  "trait"
  "impl"
  "for"
  "pub"
  "extern"
  "static"
  "import"
  "defer"
  "self"
] @keyword

; Operators
[
  "+"
  "-"
  "*"
  "/"
  "%"
  "="
  "=="
  "!="
  "<"
  ">"
  "<="
  ">="
  "&&"
  "||"
  "!"
  "&"
  "|"
  "^"
  "<<"
  ">>"
  "+="
  "-="
  "*="
  "/="
  "%="
  "&="
  "|="
  "^="
  "<<="
  ">>="
  "->"
  "::"
] @operator

; Punctuation
[
  "("
  ")"
  "{"
  "}"
  "["
  "]"
  "<"
  ">"
] @punctuation.bracket

[
  ","
  "."
  ":"
  ";"
] @punctuation.delimiter

; Types
(primitive_type) @type.builtin

(named_type
  name: (identifier) @type)

(struct_definition
  name: (identifier) @type)

(enum_definition
  name: (identifier) @type)

(trait_definition
  name: (identifier) @type)

(type_parameter
  name: (identifier) @type)

; Functions
(function_definition
  name: (identifier) @function)

(extern_function
  name: (identifier) @function)

(call_expression
  function: (identifier) @function.call)

(method_call_expression
  method: (identifier) @function.method)

(trait_method
  name: (identifier) @function.method)

; Variables and parameters
(parameter
  name: (identifier) @variable.parameter)

(let_statement
  pattern: (identifier) @variable)

(field_expression
  field: (identifier) @property)

(struct_field
  name: (identifier) @property)

(field_initializer
  name: (identifier) @property)

(enum_variant
  name: (identifier) @constant)

; Literals
(integer_literal) @number
(float_literal) @number.float
(string_literal) @string
(char_literal) @character
(escape_sequence) @string.escape
(boolean_literal) @constant.builtin

; Comments
(line_comment) @comment
(block_comment) @comment

; Lifetime
(lifetime) @label

; Identifier (fallback)
(identifier) @variable

