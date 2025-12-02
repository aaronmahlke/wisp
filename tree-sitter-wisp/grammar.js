/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

module.exports = grammar({
  name: 'wisp',

  extras: $ => [
    /\s/,
    $.line_comment,
    $.block_comment,
  ],

  word: $ => $.identifier,

  conflicts: $ => [
    [$.block, $.struct_expression],
    [$.identifier, $.field_initializer],
    [$.method_call_expression, $.field_expression],
  ],

  rules: {
    source_file: $ => repeat($._item),

    _item: $ => choice(
      $.function_definition,
      $.struct_definition,
      $.enum_definition,
      $.trait_definition,
      $.impl_block,
      $.import_statement,
      $.extern_function,
      $.extern_static,
    ),

    // Import statement
    import_statement: $ => seq(
      'import',
      $.string_literal,
    ),

    // Extern declarations
    extern_function: $ => seq(
      'extern',
      'fn',
      field('name', $.identifier),
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type))),
    ),

    extern_static: $ => seq(
      'extern',
      'static',
      field('name', $.identifier),
      ':',
      field('type', $._type),
    ),

    // Function definition
    function_definition: $ => seq(
      optional('pub'),
      'fn',
      field('name', $.identifier),
      optional(field('type_parameters', $.type_parameters)),
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type))),
      field('body', $.block),
    ),

    // Struct definition
    struct_definition: $ => seq(
      optional('pub'),
      'struct',
      field('name', $.identifier),
      optional(field('type_parameters', $.type_parameters)),
      field('body', $.struct_body),
    ),

    struct_body: $ => seq(
      '{',
      optional(seq(
        $.struct_field,
        repeat(seq(',', $.struct_field)),
        optional(','),
      )),
      '}',
    ),

    struct_field: $ => seq(
      optional('pub'),
      field('name', $.identifier),
      ':',
      field('type', $._type),
    ),

    // Enum definition
    enum_definition: $ => seq(
      optional('pub'),
      'enum',
      field('name', $.identifier),
      optional(field('type_parameters', $.type_parameters)),
      field('body', $.enum_body),
    ),

    enum_body: $ => seq(
      '{',
      optional(seq(
        $.enum_variant,
        repeat(seq(',', $.enum_variant)),
        optional(','),
      )),
      '}',
    ),

    enum_variant: $ => seq(
      field('name', $.identifier),
      optional(choice(
        seq('(', optional(seq($._type, repeat(seq(',', $._type)))), ')'),
        $.struct_body,
      )),
    ),

    // Trait definition
    trait_definition: $ => seq(
      optional('pub'),
      'trait',
      field('name', $.identifier),
      optional(field('type_parameters', $.type_parameters)),
      field('body', $.trait_body),
    ),

    trait_body: $ => seq(
      '{',
      repeat($.trait_method),
      '}',
    ),

    trait_method: $ => seq(
      'fn',
      field('name', $.identifier),
      optional(field('type_parameters', $.type_parameters)),
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type))),
      optional(field('body', $.block)),
    ),

    // Impl block
    impl_block: $ => seq(
      'impl',
      optional(field('type_parameters', $.type_parameters)),
      optional(seq(field('trait', $._type), 'for')),
      field('type', $._type),
      field('body', $.impl_body),
    ),

    impl_body: $ => seq(
      '{',
      repeat($.function_definition),
      '}',
    ),

    // Type parameters
    type_parameters: $ => seq(
      '<',
      $.type_parameter,
      repeat(seq(',', $.type_parameter)),
      optional(','),
      '>',
    ),

    type_parameter: $ => seq(
      field('name', $.identifier),
      optional(seq(':', $.trait_bounds)),
    ),

    trait_bounds: $ => seq(
      $._type,
      repeat(seq('+', $._type)),
    ),

    // Parameters
    parameter_list: $ => seq(
      '(',
      optional(seq(
        $.parameter,
        repeat(seq(',', $.parameter)),
        optional(','),
      )),
      ')',
    ),

    parameter: $ => choice(
      seq('self'),
      seq('&', 'self'),
      seq('&', 'mut', 'self'),
      seq(
        field('name', $.identifier),
        ':',
        field('type', $._type),
      ),
    ),

    // Types
    _type: $ => choice(
      $.primitive_type,
      $.named_type,
      $.reference_type,
      $.mutable_reference_type,
      $.array_type,
      $.tuple_type,
      $.function_type,
    ),

    primitive_type: $ => choice(
      'i8', 'i16', 'i32', 'i64', 'i128',
      'u8', 'u16', 'u32', 'u64', 'u128',
      'f32', 'f64',
      'bool',
      'char',
      'str',
      '()',
    ),

    named_type: $ => seq(
      field('name', choice($.identifier, 'Self')),
      optional(field('type_arguments', $.type_arguments)),
    ),

    type_arguments: $ => seq(
      '<',
      $._type,
      repeat(seq(',', $._type)),
      optional(','),
      '>',
    ),

    reference_type: $ => seq(
      '&',
      optional($.lifetime),
      field('type', $._type),
    ),

    mutable_reference_type: $ => seq(
      '&',
      optional($.lifetime),
      'mut',
      field('type', $._type),
    ),

    array_type: $ => seq(
      '[',
      field('element', $._type),
      optional(seq(';', field('size', $.integer_literal))),
      ']',
    ),

    tuple_type: $ => seq(
      '(',
      $._type,
      repeat(seq(',', $._type)),
      optional(','),
      ')',
    ),

    function_type: $ => seq(
      'fn',
      '(',
      optional(seq($._type, repeat(seq(',', $._type)))),
      ')',
      optional(seq('->', $._type)),
    ),

    lifetime: $ => seq('@', $.identifier),

    // Statements
    block: $ => seq(
      '{',
      repeat($._statement),
      optional($._expression),
      '}',
    ),

    _statement: $ => choice(
      $.let_statement,
      $.expression_statement,
      $.return_statement,
      $.defer_statement,
    ),

    let_statement: $ => seq(
      'let',
      optional('mut'),
      field('pattern', $.identifier),
      optional(seq(':', field('type', $._type))),
      optional(seq('=', field('value', $._expression))),
      ';',
    ),

    expression_statement: $ => seq(
      $._expression,
      ';',
    ),

    return_statement: $ => seq(
      'return',
      optional($._expression),
      ';',
    ),

    defer_statement: $ => seq(
      'defer',
      $._expression,
      ';',
    ),

    // Expressions
    _expression: $ => choice(
      $.identifier,
      $._literal,
      $.unary_expression,
      $.binary_expression,
      $.call_expression,
      $.method_call_expression,
      $.field_expression,
      $.index_expression,
      $.reference_expression,
      $.dereference_expression,
      $.assignment_expression,
      $.compound_assignment_expression,
      $.if_expression,
      $.while_expression,
      $.loop_expression,
      $.break_expression,
      $.continue_expression,
      $.block,
      $.struct_expression,
      $.tuple_expression,
      $.array_expression,
      $.path_expression,
      $.parenthesized_expression,
    ),

    parenthesized_expression: $ => seq('(', $._expression, ')'),

    unary_expression: $ => prec(14, choice(
      seq('-', $._expression),
      seq('!', $._expression),
    )),

    binary_expression: $ => choice(
      prec.left(4, seq($._expression, '||', $._expression)),
      prec.left(5, seq($._expression, '&&', $._expression)),
      prec.left(6, seq($._expression, '==', $._expression)),
      prec.left(6, seq($._expression, '!=', $._expression)),
      prec.left(7, seq($._expression, '<', $._expression)),
      prec.left(7, seq($._expression, '>', $._expression)),
      prec.left(7, seq($._expression, '<=', $._expression)),
      prec.left(7, seq($._expression, '>=', $._expression)),
      prec.left(8, seq($._expression, '|', $._expression)),
      prec.left(9, seq($._expression, '^', $._expression)),
      prec.left(10, seq($._expression, '&', $._expression)),
      prec.left(11, seq($._expression, '<<', $._expression)),
      prec.left(11, seq($._expression, '>>', $._expression)),
      prec.left(12, seq($._expression, '+', $._expression)),
      prec.left(12, seq($._expression, '-', $._expression)),
      prec.left(13, seq($._expression, '*', $._expression)),
      prec.left(13, seq($._expression, '/', $._expression)),
      prec.left(13, seq($._expression, '%', $._expression)),
    ),

    call_expression: $ => prec(15, seq(
      field('function', $._expression),
      field('arguments', $.argument_list),
    )),

    method_call_expression: $ => prec(15, seq(
      field('receiver', $._expression),
      '.',
      field('method', $.identifier),
      field('arguments', $.argument_list),
    )),

    field_expression: $ => prec(15, seq(
      field('value', $._expression),
      '.',
      field('field', $.identifier),
    )),

    index_expression: $ => prec(15, seq(
      field('value', $._expression),
      '[',
      field('index', $._expression),
      ']',
    )),

    reference_expression: $ => prec(14, seq(
      '&',
      optional('mut'),
      $._expression,
    )),

    dereference_expression: $ => prec(14, seq(
      '*',
      $._expression,
    )),

    assignment_expression: $ => prec.right(2, seq(
      field('left', $._expression),
      '=',
      field('right', $._expression),
    )),

    compound_assignment_expression: $ => prec.right(2, seq(
      field('left', $._expression),
      field('operator', choice('+=', '-=', '*=', '/=', '%=', '&=', '|=', '^=', '<<=', '>>=')),
      field('right', $._expression),
    )),

    argument_list: $ => seq(
      '(',
      optional(seq(
        $._expression,
        repeat(seq(',', $._expression)),
        optional(','),
      )),
      ')',
    ),

    if_expression: $ => prec.right(seq(
      'if',
      field('condition', $._expression),
      field('consequence', $.block),
      optional(seq('else', field('alternative', choice($.block, $.if_expression)))),
    )),

    while_expression: $ => seq(
      'while',
      field('condition', $._expression),
      field('body', $.block),
    ),

    loop_expression: $ => seq(
      'loop',
      field('body', $.block),
    ),

    break_expression: $ => prec.right(seq(
      'break',
      optional($._expression),
    )),

    continue_expression: $ => 'continue',

    struct_expression: $ => prec(1, seq(
      optional(seq(field('name', $.identifier), '::')),
      '{',
      optional(seq(
        $.field_initializer,
        repeat(seq(',', $.field_initializer)),
        optional(','),
      )),
      '}',
    )),

    field_initializer: $ => prec(2, seq(
      field('name', $.identifier),
      optional(seq(':', field('value', $._expression))),
    )),

    tuple_expression: $ => seq(
      '(',
      $._expression,
      ',',
      optional(seq($._expression, repeat(seq(',', $._expression)))),
      optional(','),
      ')',
    ),

    array_expression: $ => seq(
      '[',
      optional(seq(
        $._expression,
        repeat(seq(',', $._expression)),
        optional(','),
      )),
      ']',
    ),

    path_expression: $ => seq(
      $.identifier,
      repeat1(seq('::', $.identifier)),
    ),

    // Literals
    _literal: $ => choice(
      $.integer_literal,
      $.float_literal,
      $.string_literal,
      $.char_literal,
      $.boolean_literal,
    ),

    integer_literal: $ => token(choice(
      /[0-9][0-9_]*/,
      /0x[0-9a-fA-F_]+/,
      /0o[0-7_]+/,
      /0b[01_]+/,
    )),

    float_literal: $ => token(
      /[0-9][0-9_]*\.[0-9][0-9_]*([eE][+-]?[0-9_]+)?/,
    ),

    string_literal: $ => seq(
      '"',
      repeat(choice(
        $.escape_sequence,
        /[^"\\]+/,
      )),
      '"',
    ),

    char_literal: $ => seq(
      "'",
      choice(
        $.escape_sequence,
        /[^'\\]/,
      ),
      "'",
    ),

    escape_sequence: $ => token.immediate(seq(
      '\\',
      choice(
        /[nrt\\'"0]/,
        /x[0-9a-fA-F]{2}/,
        /u\{[0-9a-fA-F]+\}/,
      ),
    )),

    boolean_literal: $ => choice('true', 'false'),

    // Identifier
    identifier: $ => /[a-zA-Z_][a-zA-Z0-9_]*/,

    // Comments
    line_comment: $ => token(seq('//', /.*/)),

    block_comment: $ => token(seq(
      '/*',
      /[^*]*\*+([^/*][^*]*\*+)*/,
      '/',
    )),
  },
});

