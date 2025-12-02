use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::collections::HashSet;
use std::process::Command;
use wisp_lexer::{Lexer, Token};
use wisp_parser::Parser;
use wisp_ast::{SourceFile, Item};
use wisp_hir::Resolver;
use wisp_types::TypeChecker;
use wisp_borrowck::BorrowChecker;
use wisp_mir::lower_program;
use wisp_codegen::Codegen;

fn print_usage() {
    eprintln!("Wisp Compiler");
    eprintln!();
    eprintln!("Usage: wisp <command> <file.ws>");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  run <file>      Compile and run the program");
    eprintln!("  build <file>    Compile to executable");
    eprintln!("  lex <file>      Show lexer output (tokens)");
    eprintln!("  parse <file>    Show parser output (AST)");
    eprintln!("  resolve <file>  Show name resolution (HIR)");
    eprintln!("  check <file>    Show type checking output");
    eprintln!("  borrow <file>   Show borrow checking output");
    eprintln!("  mir <file>      Show MIR output");
    eprintln!("  emit-obj <file> Emit object file only");
    eprintln!("  help            Show this help message");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  wisp run examples/hello.ws");
    eprintln!("  wisp build examples/hello.ws");
    eprintln!("  wisp parse examples/hello.ws");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    // Handle help
    if args[1] == "help" || args[1] == "--help" || args[1] == "-h" {
        print_usage();
        std::process::exit(0);
    }

    // All commands require a file argument
    let commands = ["run", "build", "lex", "parse", "resolve", "check", "borrow", "mir", "emit-obj"];
    
    let (mode, file_path) = if commands.contains(&args[1].as_str()) {
        if args.len() < 3 {
            eprintln!("Usage: wisp {} <file.ws>", args[1]);
            std::process::exit(1);
        }
        (args[1].as_str(), &args[2])
    } else if args[1].starts_with("--") {
        // Support legacy --command syntax
        let cmd = args[1].trim_start_matches("--");
        if commands.contains(&cmd) {
            if args.len() < 3 {
                eprintln!("Usage: wisp {} <file.ws>", cmd);
                std::process::exit(1);
            }
            (cmd, &args[2])
        } else {
            eprintln!("Unknown command: {}", args[1]);
            print_usage();
            std::process::exit(1);
        }
    } else {
        // Default: treat argument as file and run it
        ("run", &args[1])
    };

    let source = match fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", file_path, e);
            std::process::exit(1);
        }
    };

    match mode {
        "run" => run_and_execute(&source, file_path),
        "build" => run_build(&source, file_path),
        "lex" => run_lexer(&source, file_path),
        "parse" => run_parser(&source, file_path),
        "resolve" => run_resolver(&source, file_path),
        "check" => run_type_check(&source, file_path),
        "borrow" => run_borrow_check(&source, file_path),
        "mir" => run_mir(&source, file_path),
        "emit-obj" => run_codegen(&source, file_path),
        _ => unreachable!(),
    }
}

/// Get the build directory (creates .build in current working directory)
fn get_build_dir() -> PathBuf {
    let build_dir = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".build");
    let _ = fs::create_dir_all(&build_dir);
    build_dir
}

/// Compile and run a Wisp program
fn run_and_execute(source: &str, file_path: &str) {
    let build_dir = get_build_dir();
    
    let file_stem = Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("program");
    
    let obj_path = build_dir.join(format!("{}.o", file_stem));
    let exe_path = build_dir.join(file_stem);
    
    // Compile to object file
    if let Err(()) = compile_to_object(source, file_path, &obj_path) {
        std::process::exit(1);
    }
    
    // Link with cc
    let link_status = Command::new("cc")
        .arg(&obj_path)
        .arg("-o")
        .arg(&exe_path)
        .status();
    
    match link_status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            eprintln!("Linking failed with exit code: {:?}", status.code());
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Failed to run linker: {}", e);
            std::process::exit(1);
        }
    }
    
    // Execute the program
    let run_status = Command::new(&exe_path)
        .status();
    
    match run_status {
        Ok(status) => {
            std::process::exit(status.code().unwrap_or(1));
        }
        Err(e) => {
            eprintln!("Failed to execute program: {}", e);
            std::process::exit(1);
        }
    }
}

/// Compile a Wisp program to an executable
fn run_build(source: &str, file_path: &str) {
    let build_dir = get_build_dir();
    
    let file_stem = Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("program");
    
    let obj_path = build_dir.join(format!("{}.o", file_stem));
    let exe_path = build_dir.join(file_stem);
    
    // Compile to object file
    if let Err(()) = compile_to_object(source, file_path, &obj_path) {
        std::process::exit(1);
    }
    
    // Link with cc
    let link_status = Command::new("cc")
        .arg(&obj_path)
        .arg("-o")
        .arg(&exe_path)
        .status();
    
    match link_status {
        Ok(status) if status.success() => {
            println!("Built: {}", exe_path.display());
            // Clean up object file
            let _ = fs::remove_file(&obj_path);
        }
        Ok(status) => {
            eprintln!("Linking failed with exit code: {:?}", status.code());
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Failed to run linker: {}", e);
            std::process::exit(1);
        }
    }
}

/// Compile source to an object file
fn compile_to_object(source: &str, file_path: &str, output_path: &Path) -> Result<(), ()> {
    // Run full frontend pipeline
    let typed = run_frontend(source, file_path)?;
    
    // Lower to MIR
    let mir = lower_program(&typed);
    
    // Generate code
    let mut codegen = match Codegen::new() {
        Ok(cg) => cg,
        Err(e) => {
            eprintln!("Codegen error: {}", e);
            return Err(());
        }
    };
    
    if let Err(e) = codegen.compile(&mir) {
        eprintln!("Compilation error: {}", e);
        return Err(());
    }
    
    // Emit object file
    let obj_bytes = codegen.finish();
    
    if let Err(e) = fs::write(output_path, &obj_bytes) {
        eprintln!("Failed to write object file: {}", e);
        return Err(());
    }
    
    Ok(())
}

/// Parse a file and recursively handle imports
fn parse_with_imports(source: &str, file_path: &str) -> Result<SourceFile, String> {
    let base_dir = Path::new(file_path).parent().unwrap_or(Path::new("."));
    let mut visited = HashSet::new();
    visited.insert(PathBuf::from(file_path).canonicalize().unwrap_or(PathBuf::from(file_path)));
    
    parse_with_imports_recursive(source, base_dir, &mut visited)
}

fn parse_with_imports_recursive(
    source: &str,
    base_dir: &Path,
    visited: &mut HashSet<PathBuf>,
) -> Result<SourceFile, String> {
    let ast = Parser::parse(source).map_err(|e| format!("Parse error: {}", e))?;
    
    let mut all_items = Vec::new();
    
    for item in ast.items {
        match item {
            Item::Import(import) => {
                // Resolve import path relative to base_dir
                let import_path = base_dir.join(&import.path);
                let import_path = if import_path.extension().is_none() {
                    import_path.with_extension("ws")
                } else {
                    import_path
                };
                
                let canonical = import_path.canonicalize()
                    .map_err(|e| format!("Cannot find import '{}': {}", import.path, e))?;
                
                // Skip if already imported
                if visited.contains(&canonical) {
                    continue;
                }
                visited.insert(canonical.clone());
                
                // Read and parse the imported file
                let import_source = fs::read_to_string(&import_path)
                    .map_err(|e| format!("Cannot read import '{}': {}", import.path, e))?;
                
                let import_dir = import_path.parent().unwrap_or(Path::new("."));
                let imported_ast = parse_with_imports_recursive(&import_source, import_dir, visited)?;
                
                // Add all items from the imported file
                all_items.extend(imported_ast.items);
            }
            other => all_items.push(other),
        }
    }
    
    Ok(SourceFile { items: all_items })
}

fn run_lexer(source: &str, file_path: &str) {
    println!("=== Lexer Output for {} ===\n", file_path);
    
    match Lexer::tokenize(source) {
        Ok(tokens) => {
            println!("{:<6} {:<10} {:<20} {}", "SPAN", "LENGTH", "TOKEN TYPE", "VALUE");
            println!("{}", "-".repeat(60));
            
            for spanned in &tokens {
                let span_str = format!("{}..{}", spanned.span.start, spanned.span.end);
                let len = spanned.span.end - spanned.span.start;
                let token_type = token_type_name(&spanned.token);
                let value = format!("{}", spanned.token);
                
                println!("{:<6} {:<10} {:<20} {}", span_str, len, token_type, value);
            }
            
            println!("\n=== Summary ===");
            println!("Total tokens: {}", tokens.len());
            
            // Count by type
            let keywords = tokens.iter().filter(|t| is_keyword(&t.token)).count();
            let idents = tokens.iter().filter(|t| matches!(t.token, Token::Ident(_))).count();
            let literals = tokens.iter().filter(|t| is_literal(&t.token)).count();
            let operators = tokens.iter().filter(|t| is_operator(&t.token)).count();
            let delimiters = tokens.iter().filter(|t| is_delimiter(&t.token)).count();
            
            println!("  Keywords:   {}", keywords);
            println!("  Identifiers: {}", idents);
            println!("  Literals:   {}", literals);
            println!("  Operators:  {}", operators);
            println!("  Delimiters: {}", delimiters);
        }
        Err(e) => {
            eprintln!("Lexer error: {}", e);
            std::process::exit(1);
        }
    }
}

fn run_parser(source: &str, file_path: &str) {
    println!("=== Parser Output for {} ===\n", file_path);
    
    match Parser::parse(source) {
        Ok(ast) => {
            println!("{}", ast.pretty_print());
            
            println!("=== Summary ===");
            let fn_count = ast.items.iter().filter(|i| matches!(i, wisp_ast::Item::Function(_))).count();
            let struct_count = ast.items.iter().filter(|i| matches!(i, wisp_ast::Item::Struct(_))).count();
            println!("Functions: {}", fn_count);
            println!("Structs:   {}", struct_count);
        }
        Err(e) => {
            eprintln!("Parse error: {}", e);
            
            // Show context around error
            let lines: Vec<&str> = source.lines().collect();
            let mut char_count = 0;
            for (line_num, line) in lines.iter().enumerate() {
                let line_start = char_count;
                let line_end = char_count + line.len();
                
                if e.span.start >= line_start && e.span.start <= line_end {
                    eprintln!("\n  {} | {}", line_num + 1, line);
                    let col = e.span.start - line_start;
                    eprintln!("  {} | {}^", " ".repeat((line_num + 1).to_string().len()), " ".repeat(col));
                    break;
                }
                
                char_count = line_end + 1; // +1 for newline
            }
            
            std::process::exit(1);
        }
    }
}

fn token_type_name(token: &Token) -> &'static str {
    match token {
        Token::Fn | Token::Let | Token::Mut | Token::If | Token::Else |
        Token::While | Token::For | Token::In | Token::Return | Token::Struct |
        Token::Enum | Token::Trait | Token::Impl | Token::Pub | Token::Const |
        Token::True | Token::False | Token::Match | Token::Defer | Token::Import |
        Token::As | Token::Type | Token::Where | Token::SelfLower | Token::SelfUpper |
        Token::Extern | Token::Static => "KEYWORD",
        
        Token::IntLiteral(_) => "INT",
        Token::FloatLiteral(_) => "FLOAT",
        Token::StringLiteral(_) => "STRING",
        Token::CharLiteral(_) => "CHAR",
        
        Token::Ident(_) => "IDENT",
        
        Token::Plus | Token::Minus | Token::Star | Token::Slash | Token::Percent |
        Token::Eq | Token::EqEq | Token::NotEq | Token::Lt | Token::Gt |
        Token::LtEq | Token::GtEq | Token::AndAnd | Token::OrOr | Token::Not |
        Token::Amp | Token::Pipe | Token::Caret | Token::PlusEq | Token::MinusEq |
        Token::StarEq | Token::SlashEq | Token::Question => "OPERATOR",
        
        Token::LParen | Token::RParen | Token::LBrace | Token::RBrace |
        Token::LBracket | Token::RBracket => "DELIMITER",
        
        Token::Comma | Token::Colon | Token::ColonColon | Token::Semi |
        Token::Dot | Token::DotDot | Token::Arrow | Token::At => "PUNCTUATION",
        
        Token::Eof => "EOF",
    }
}

fn is_keyword(token: &Token) -> bool {
    matches!(token_type_name(token), "KEYWORD")
}

fn is_literal(token: &Token) -> bool {
    matches!(token, Token::IntLiteral(_) | Token::FloatLiteral(_) | 
             Token::StringLiteral(_) | Token::CharLiteral(_))
}

fn is_operator(token: &Token) -> bool {
    matches!(token_type_name(token), "OPERATOR")
}

fn is_delimiter(token: &Token) -> bool {
    matches!(token_type_name(token), "DELIMITER" | "PUNCTUATION")
}

fn run_resolver(source: &str, file_path: &str) {
    println!("=== Name Resolution for {} ===\n", file_path);
    
    // First parse with imports
    let ast = match parse_with_imports(source, file_path) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    
    // Then resolve
    match Resolver::resolve(&ast) {
        Ok(hir) => {
            println!("{}", hir.pretty_print());
        }
        Err(errors) => {
            eprintln!("Resolution errors:");
            for e in &errors {
                eprintln!("  {}", e);
                show_error_context(source, e.span);
            }
            std::process::exit(1);
        }
    }
}

fn run_type_check(source: &str, file_path: &str) {
    println!("=== Type Check for {} ===\n", file_path);
    
    // Parse with imports
    let ast = match parse_with_imports(source, file_path) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    
    // Resolve
    let hir = match Resolver::resolve(&ast) {
        Ok(hir) => hir,
        Err(errors) => {
            eprintln!("Resolution errors:");
            for e in &errors {
                eprintln!("  {}", e);
                show_error_context(source, e.span);
            }
            std::process::exit(1);
        }
    };
    
    // Type check
    match TypeChecker::check(&hir) {
        Ok(typed) => {
            println!("{}", typed.pretty_print());
            println!("Type checking successful!");
        }
        Err(errors) => {
            eprintln!("Type errors:");
            for e in &errors {
                eprintln!("  {}", e);
                show_error_context(source, e.span);
            }
            std::process::exit(1);
        }
    }
}

fn run_borrow_check(source: &str, file_path: &str) {
    println!("=== Borrow Check for {} ===\n", file_path);
    
    // Parse with imports
    let ast = match parse_with_imports(source, file_path) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    
    // Resolve
    let hir = match Resolver::resolve(&ast) {
        Ok(hir) => hir,
        Err(errors) => {
            eprintln!("Resolution errors:");
            for e in &errors {
                eprintln!("  {}", e);
                show_error_context(source, e.span);
            }
            std::process::exit(1);
        }
    };
    
    // Type check
    let typed = match TypeChecker::check(&hir) {
        Ok(typed) => typed,
        Err(errors) => {
            eprintln!("Type errors:");
            for e in &errors {
                eprintln!("  {}", e);
                show_error_context(source, e.span);
            }
            std::process::exit(1);
        }
    };
    
    // Borrow check
    let checker = BorrowChecker::new(&typed);
    match checker.check() {
        Ok(()) => {
            println!("{}", typed.pretty_print());
            println!("Borrow checking successful!");
        }
        Err(errors) => {
            eprintln!("Borrow check errors:");
            for e in &errors {
                eprintln!("  {}", e.message);
                show_error_context(source, e.span);
                for (note, note_span) in &e.notes {
                    eprintln!("  note: {}", note);
                    show_error_context(source, *note_span);
                }
            }
            std::process::exit(1);
        }
    }
}

fn run_mir(source: &str, file_path: &str) {
    println!("=== MIR for {} ===\n", file_path);
    
    // Run full frontend pipeline
    let typed = match run_frontend(source, file_path) {
        Ok(typed) => typed,
        Err(()) => std::process::exit(1),
    };
    
    // Lower to MIR
    let mir = lower_program(&typed);
    println!("{}", mir.pretty_print());
}

fn run_codegen(source: &str, file_path: &str) {
    println!("=== Compiling {} ===\n", file_path);
    
    // Run full frontend pipeline
    let typed = match run_frontend(source, file_path) {
        Ok(typed) => typed,
        Err(()) => std::process::exit(1),
    };
    
    // Lower to MIR
    let mir = lower_program(&typed);
    println!("MIR generated: {} functions", mir.functions.len());
    
    // Generate code
    let mut codegen = match Codegen::new() {
        Ok(cg) => cg,
        Err(e) => {
            eprintln!("Codegen error: {}", e);
            std::process::exit(1);
        }
    };
    
    if let Err(e) = codegen.compile(&mir) {
        eprintln!("Compilation error: {}", e);
        std::process::exit(1);
    }
    
    // Emit object file
    let obj_bytes = codegen.finish();
    
    let output_path = Path::new(file_path).with_extension("o");
    if let Err(e) = fs::write(&output_path, &obj_bytes) {
        eprintln!("Failed to write object file: {}", e);
        std::process::exit(1);
    }
    
    println!("Wrote {} bytes to {}", obj_bytes.len(), output_path.display());
    println!("\nTo link: cc {} -o {}", 
        output_path.display(),
        Path::new(file_path).with_extension("").display()
    );
}

fn run_frontend(source: &str, file_path: &str) -> Result<wisp_types::TypedProgram, ()> {
    // Parse with imports
    let ast = match parse_with_imports(source, file_path) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("{}", e);
            return Err(());
        }
    };
    
    // Resolve
    let hir = match Resolver::resolve(&ast) {
        Ok(hir) => hir,
        Err(errors) => {
            eprintln!("Resolution errors:");
            for e in &errors {
                eprintln!("  {}", e);
                show_error_context(source, e.span);
            }
            return Err(());
        }
    };
    
    // Type check
    let typed = match TypeChecker::check(&hir) {
        Ok(typed) => typed,
        Err(errors) => {
            eprintln!("Type errors:");
            for e in &errors {
                eprintln!("  {}", e);
                show_error_context(source, e.span);
            }
            return Err(());
        }
    };
    
    // Borrow check
    let checker = BorrowChecker::new(&typed);
    if let Err(errors) = checker.check() {
        eprintln!("Borrow check errors:");
        for e in &errors {
            eprintln!("  {}", e.message);
            show_error_context(source, e.span);
            for (note, note_span) in &e.notes {
                eprintln!("  note: {}", note);
                show_error_context(source, *note_span);
            }
        }
        return Err(());
    }
    
    Ok(typed)
}

fn show_error_context(source: &str, span: wisp_lexer::Span) {
    let lines: Vec<&str> = source.lines().collect();
    let mut char_count = 0;
    for (line_num, line) in lines.iter().enumerate() {
        let line_start = char_count;
        let line_end = char_count + line.len();
        
        if span.start >= line_start && span.start <= line_end {
            eprintln!("\n  {} | {}", line_num + 1, line);
            let col = span.start - line_start;
            eprintln!("  {} | {}^", " ".repeat((line_num + 1).to_string().len()), " ".repeat(col));
            break;
        }
        
        char_count = line_end + 1; // +1 for newline
    }
}
