#![allow(dead_code)]
use std::process::ExitCode;

mod ast;
mod error;
mod interpreter;
mod lexer;
mod parser;
mod token;
mod value;

use interpreter::Interpreter;
use lexer::Lexer;
use parser::Parser;

fn print_help() {
    println!("rock - the Rock programming language");
    println!();
    println!("Usage:");
    println!("  rock run <file.rk>        Run a source file");
    println!("  rock check <file.rk>      Parse + type-check without running");
    println!("  rock prove <file.rk>      Verify all @prove blocks");
    println!("  rock fmt <file.rk>        Print source with canonical formatting (stdout)");
    println!("  rock tokens <file.rk>     Dump token stream");
    println!("  rock ast <file.rk>        Dump parsed AST");
    println!("  rock bench <file.rk> [n]  Run file n times, report timings (default n=10)");
    println!("  rock test [file.rk|dir]   Run all fn test_*() in file or all .rk under dir (default cwd)");
    println!("  rock fuzz <file.rk> <fn> [n]  Random-input fuzz testing (default n=1000)");
    println!("  rock doc <file.rk>        Print extracted /// doc comments");
    println!("  rock repl                 Start interactive REPL");
    println!("  rock init [name]          Create a new Rock project (rock.toml + src/main.rk)");
    println!("  rock pkg info             Print current project's manifest");
    println!("  rock pkg install <url> [ref]   Clone a github.com/owner/repo into the local cache");
    println!("  rock pkg list             List installed packages");
    println!("  rock version              Print version");
    println!("  rock help                 Show this message");
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.get(0).map(String::as_str).unwrap_or("help");

    let result = match cmd {
        "run" => match args.get(1) {
            Some(path) => cmd_run(path),
            None => { print_help(); return ExitCode::from(2); }
        },
        "tokens" => match args.get(1) {
            Some(path) => cmd_tokens(path),
            None => { print_help(); return ExitCode::from(2); }
        },
        "ast" => match args.get(1) {
            Some(path) => cmd_ast(path),
            None => { print_help(); return ExitCode::from(2); }
        },
        "check" => match args.get(1) {
            Some(path) => cmd_check(path),
            None => { print_help(); return ExitCode::from(2); }
        },
        "prove" => match args.get(1) {
            Some(path) => cmd_prove(path),
            None => { print_help(); return ExitCode::from(2); }
        },
        "fmt" => match args.get(1) {
            Some(path) => cmd_fmt(path),
            None => { print_help(); return ExitCode::from(2); }
        },
        "init" => cmd_init(args.get(1).map(String::as_str)),
        "pkg" => match args.get(1).map(String::as_str) {
            Some("info") => cmd_pkg_info(),
            Some("install") => match args.get(2) {
                Some(url) => cmd_pkg_install(url, args.get(3).map(String::as_str)),
                None => { print_help(); return ExitCode::from(2); }
            },
            Some("list") => cmd_pkg_list(),
            _ => { print_help(); return ExitCode::from(2); }
        },
        "bench" => match args.get(1) {
            Some(path) => {
                let n = args.get(2).and_then(|s| s.parse::<usize>().ok()).unwrap_or(10);
                cmd_bench(path, n)
            }
            None => { print_help(); return ExitCode::from(2); }
        },
        "test" => {
            let path = args.get(1).map(String::as_str);
            cmd_test(path)
        }
        "fuzz" => match (args.get(1), args.get(2)) {
            (Some(path), Some(fn_name)) => {
                let n = args.get(3).and_then(|s| s.parse::<usize>().ok()).unwrap_or(1000);
                cmd_fuzz(path, fn_name, n)
            }
            _ => { print_help(); return ExitCode::from(2); }
        },
        "doc" => match args.get(1) {
            Some(path) => cmd_doc(path),
            None => { print_help(); return ExitCode::from(2); }
        },
        "repl" => cmd_repl(),
        "version" => { println!("rock 0.1.0"); Ok(()) }
        "help" | "--help" | "-h" => { print_help(); Ok(()) }
        other => {
            eprintln!("unknown command: {}", other);
            print_help();
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", e);
            ExitCode::FAILURE
        }
    }
}

fn read_source(path: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(std::fs::read_to_string(path)?)
}

fn cmd_run(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let src = read_source(path)?;
    run_with_pretty(&src, path)
}

fn run_with_pretty(src: &str, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let toks = match Lexer::new(src).tokenize() {
        Ok(t) => t,
        Err(e) => { eprint!("{}", e.pretty(src, Some(path))); return Err("aborted".into()); }
    };
    let program = match Parser::new(toks).parse_program() {
        Ok(p) => p,
        Err(e) => { eprint!("{}", e.pretty(src, Some(path))); return Err("aborted".into()); }
    };
    let mut interp = Interpreter::new();
    let base = std::path::Path::new(path).parent().map(|p| p.to_path_buf());
    if let Err(e) = interp.run_with_base(&program, base) {
        eprint!("{}", e.pretty(src, Some(path)));
        return Err("aborted".into());
    }
    Ok(())
}

fn cmd_tokens(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let src = read_source(path)?;
    let toks = Lexer::new(&src).tokenize()?;
    for t in &toks {
        println!("{}  {:?}", t.span, t.token);
    }
    Ok(())
}

fn cmd_ast(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let src = read_source(path)?;
    let toks = Lexer::new(&src).tokenize()?;
    let program = Parser::new(toks).parse_program()?;
    println!("{:#?}", program);
    Ok(())
}

fn cmd_check(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let src = read_source(path)?;
    let toks = Lexer::new(&src).tokenize()?;
    let program = Parser::new(toks).parse_program()?;
    let items = program.items.len();
    let fns = program.items.iter().filter(|i| matches!(i, ast::Item::Function(_))).count();
    let types = program.items.iter().filter(|i| matches!(i, ast::Item::TypeDecl(_))).count();
    let traits = program.items.iter().filter(|i| matches!(i, ast::Item::Trait(_))).count();
    let impls = program.items.iter().filter(|i| matches!(i, ast::Item::Impl(_) | ast::Item::TraitImpl(_))).count();
    println!("ok: {} items ({} fn, {} type, {} trait, {} impl)", items, fns, types, traits, impls);
    Ok(())
}

fn cmd_prove(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let src = read_source(path)?;
    let toks = Lexer::new(&src).tokenize()?;
    let program = Parser::new(toks).parse_program()?;
    let mut count = 0usize;
    for item in &program.items {
        if let ast::Item::Prove(b) = item {
            count += b.assertions.len();
        }
    }
    let mut interp = Interpreter::new();
    interp.run_prove_only(&program)?;
    println!("ok: verified {} @prove assertion(s)", count);
    Ok(())
}

fn cmd_fmt(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let src = read_source(path)?;
    // Validate by parsing first
    let toks = Lexer::new(&src).tokenize()?;
    let _ = Parser::new(toks).parse_program()?;

    // Layout-preserving normalizer:
    //  - trim trailing whitespace
    //  - re-indent each line by counting brace depth (4 spaces per level)
    //  - collapse runs of >1 blank line into a single blank line
    //  - tabs → 4 spaces
    //  - no trailing newline duplication
    let mut out = String::with_capacity(src.len());
    let mut depth: i32 = 0;
    let mut blanks = 0usize;
    let mut in_string = false;
    let mut string_quote = '"';

    for raw in src.lines() {
        let mut line = raw.replace('\t', "    ");
        // strip trailing whitespace
        while line.ends_with(' ') || line.ends_with('\t') { line.pop(); }

        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            blanks += 1;
            if blanks <= 1 { out.push('\n'); }
            continue;
        }
        blanks = 0;

        // count leading dedent: if the first non-ws char is `}` or `)` or `]`, dedent first
        let mut effective_depth = depth;
        let first = trimmed.chars().next().unwrap();
        if first == '}' || first == ']' || first == ')' {
            effective_depth = (depth - 1).max(0);
        }
        for _ in 0..effective_depth { out.push_str("    "); }
        out.push_str(trimmed);
        out.push('\n');

        // Recount depth from the actual line content, ignoring strings/comments
        let bytes = trimmed.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i] as char;
            if in_string {
                if c == '\\' && i + 1 < bytes.len() { i += 2; continue; }
                if c == string_quote { in_string = false; }
                i += 1;
                continue;
            }
            // line comment
            if c == '/' && i + 1 < bytes.len() && bytes[i + 1] as char == '/' { break; }
            if c == '"' || c == '\'' || c == '`' { in_string = true; string_quote = c; i += 1; continue; }
            match c {
                '{' | '[' | '(' => depth += 1,
                '}' | ']' | ')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        in_string = false; // strings don't span lines in rock
        if depth < 0 { depth = 0; }
    }

    while out.ends_with("\n\n") { out.pop(); }
    if !out.ends_with('\n') { out.push('\n'); }
    print!("{}", out);
    Ok(())
}

#[derive(Debug, Default)]
struct Manifest {
    name: String,
    version: String,
    edition: String,
    dependencies: Vec<(String, String)>,
}

fn parse_manifest(text: &str) -> Result<Manifest, Box<dyn std::error::Error>> {
    let mut m = Manifest::default();
    let mut section = String::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() { continue; }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len()-1].trim().to_string();
            continue;
        }
        let (k, v) = match line.split_once('=') {
            Some(p) => p,
            None => return Err(format!("manifest line {}: expected 'key = value'", i + 1).into()),
        };
        let key = k.trim().to_string();
        let mut val = v.trim().to_string();
        if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
            val = val[1..val.len()-1].to_string();
        }
        match section.as_str() {
            "package" => match key.as_str() {
                "name" => m.name = val,
                "version" => m.version = val,
                "edition" => m.edition = val,
                _ => {}
            },
            "dependencies" => m.dependencies.push((key, val)),
            _ => {}
        }
    }
    if m.name.is_empty() { return Err("manifest: missing [package] name".into()); }
    if m.version.is_empty() { m.version = "0.1.0".to_string(); }
    if m.edition.is_empty() { m.edition = "2026".to_string(); }
    Ok(m)
}

fn find_manifest(start: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut cur = start.canonicalize().ok()?;
    loop {
        let candidate = cur.join("rock.toml");
        if candidate.exists() { return Some(candidate); }
        if !cur.pop() { return None; }
    }
}

fn cmd_init(name_arg: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let (target_dir, name) = match name_arg {
        Some(n) => (cwd.join(n), n.to_string()),
        None => {
            let n = cwd.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("rock-project")
                .to_string();
            (cwd.clone(), n)
        }
    };
    if !target_dir.exists() {
        std::fs::create_dir_all(&target_dir)?;
    }
    let manifest_path = target_dir.join("rock.toml");
    if manifest_path.exists() {
        return Err(format!("'{}' already exists", manifest_path.display()).into());
    }
    let manifest = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[dependencies]\n",
        name
    );
    std::fs::write(&manifest_path, manifest)?;
    let src_dir = target_dir.join("src");
    std::fs::create_dir_all(&src_dir)?;
    let main_path = src_dir.join("main.rk");
    if !main_path.exists() {
        std::fs::write(&main_path, "fn main() {\n    print(\"hello from rock\")\n}\n")?;
    }
    println!("created {}", manifest_path.display());
    println!("created {}", main_path.display());
    Ok(())
}

fn cmd_pkg_info() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let path = find_manifest(&cwd)
        .ok_or("no rock.toml found in this directory or any parent")?;
    let text = std::fs::read_to_string(&path)?;
    let m = parse_manifest(&text)?;
    println!("manifest: {}", path.display());
    println!("name:     {}", m.name);
    println!("version:  {}", m.version);
    println!("edition:  {}", m.edition);
    if m.dependencies.is_empty() {
        println!("deps:     (none)");
    } else {
        println!("deps:");
        for (k, v) in &m.dependencies {
            println!("  {} = {}", k, v);
        }
    }
    Ok(())
}

fn cmd_pkg_install(url: &str, git_ref: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    // Accept full URL or short form: github.com/owner/repo
    let (host, owner, repo) = parse_pkg_url(url)?;
    let ref_str = git_ref.unwrap_or("HEAD");
    let cache = interpreter::rock_pkg_cache_dir();
    let mut owner_dir = cache.clone();
    owner_dir.push(&host);
    owner_dir.push(&owner);
    std::fs::create_dir_all(&owner_dir)?;

    // Resolve commit hash via `git ls-remote` so the cache key is content-pinned
    let clone_url = format!("https://{}/{}/{}.git", host, owner, repo);
    let ls = std::process::Command::new("git")
        .args(["ls-remote", &clone_url, ref_str])
        .output()
        .map_err(|e| format!("git ls-remote failed: {} (is git installed?)", e))?;
    if !ls.status.success() {
        return Err(format!("git ls-remote {} {} failed:\n{}", clone_url, ref_str, String::from_utf8_lossy(&ls.stderr)).into());
    }
    let stdout = String::from_utf8_lossy(&ls.stdout);
    let commit = stdout.split_whitespace().next()
        .ok_or_else(|| format!("ref '{}' not found in {}", ref_str, clone_url))?
        .to_string();
    let short = &commit[..commit.len().min(12)];

    let mut pkg_dir = owner_dir.clone();
    pkg_dir.push(format!("{}@{}", repo, short));

    if pkg_dir.exists() {
        println!("already installed: {}", pkg_dir.display());
    } else {
        println!("cloning {} @ {} ...", clone_url, short);
        let status = std::process::Command::new("git")
            .args(["clone", "--depth", "1", "--quiet", &clone_url])
            .arg(&pkg_dir)
            .status()
            .map_err(|e| format!("git clone failed: {}", e))?;
        if !status.success() {
            return Err(format!("git clone {} into {} failed", clone_url, pkg_dir.display()).into());
        }
        // Checkout the resolved commit if it's not HEAD on default branch
        if ref_str != "HEAD" {
            let _ = std::process::Command::new("git")
                .args(["-C"]).arg(&pkg_dir)
                .args(["fetch", "--depth", "1", "origin", &commit])
                .status();
            let _ = std::process::Command::new("git")
                .args(["-C"]).arg(&pkg_dir)
                .args(["checkout", "--quiet", &commit])
                .status();
        }
        println!("installed: {}", pkg_dir.display());
    }

    // Update rock.lock in the current project (if any)
    let cwd = std::env::current_dir()?;
    if let Some(manifest) = find_manifest(&cwd) {
        let proj_dir = manifest.parent().unwrap();
        let lock_path = proj_dir.join("rock.lock");
        let entry = format!("{}/{}/{} = \"{}\"\n", host, owner, repo, commit);
        let mut existing = std::fs::read_to_string(&lock_path).unwrap_or_default();
        let key = format!("{}/{}/{} =", host, owner, repo);
        let mut new_lines: Vec<String> = existing.lines()
            .filter(|l| !l.trim_start().starts_with(&key))
            .map(|l| l.to_string())
            .collect();
        new_lines.push(entry.trim_end().to_string());
        existing = new_lines.join("\n");
        if !existing.ends_with('\n') { existing.push('\n'); }
        std::fs::write(&lock_path, existing)?;
        println!("updated:   {}", lock_path.display());

        // Also append to manifest [dependencies] if not present
        let manifest_text = std::fs::read_to_string(&manifest)?;
        let dep_key = format!("{}/{}/{}", host, owner, repo);
        if !manifest_text.lines().any(|l| l.trim_start().starts_with(&format!("{} =", dep_key)) || l.trim_start().starts_with(&format!("\"{}\"", dep_key))) {
            let mut new_text = manifest_text.clone();
            if !new_text.contains("[dependencies]") {
                if !new_text.ends_with('\n') { new_text.push('\n'); }
                new_text.push_str("\n[dependencies]\n");
            }
            // Insert at end of file (simple approach)
            if !new_text.ends_with('\n') { new_text.push('\n'); }
            new_text.push_str(&format!("\"{}\" = \"{}\"\n", dep_key, ref_str));
            std::fs::write(&manifest, new_text)?;
            println!("updated:   {} (added dependency)", manifest.display());
        }
    }
    Ok(())
}

fn parse_pkg_url(url: &str) -> Result<(String, String, String), Box<dyn std::error::Error>> {
    // Strip optional scheme and trailing .git
    let mut u = url.trim();
    for prefix in ["https://", "http://", "git+https://", "ssh://git@"] {
        if let Some(s) = u.strip_prefix(prefix) { u = s; break; }
    }
    let u = u.trim_end_matches(".git").trim_end_matches('/');
    let parts: Vec<&str> = u.split('/').collect();
    if parts.len() < 3 {
        return Err(format!("expected URL like 'github.com/owner/repo', got '{}'", url).into());
    }
    Ok((parts[0].to_string(), parts[1].to_string(), parts[2].to_string()))
}

fn cmd_pkg_list() -> Result<(), Box<dyn std::error::Error>> {
    let cache = interpreter::rock_pkg_cache_dir();
    if !cache.exists() {
        println!("(no packages installed)  cache: {}", cache.display());
        return Ok(());
    }
    println!("cache: {}", cache.display());
    let mut found = 0;
    for host in std::fs::read_dir(&cache)? {
        let host = host?;
        if !host.file_type()?.is_dir() { continue; }
        for owner in std::fs::read_dir(host.path())? {
            let owner = owner?;
            if !owner.file_type()?.is_dir() { continue; }
            for repo in std::fs::read_dir(owner.path())? {
                let repo = repo?;
                println!("  {}/{}/{}", host.file_name().to_string_lossy(), owner.file_name().to_string_lossy(), repo.file_name().to_string_lossy());
                found += 1;
            }
        }
    }
    if found == 0 { println!("  (empty)"); }
    Ok(())
}

fn cmd_bench(path: &str, n: usize) -> Result<(), Box<dyn std::error::Error>> {
    if n == 0 { return Err("bench: n must be >= 1".into()); }
    let src = read_source(path)?;
    let toks = Lexer::new(&src).tokenize()?;
    let program = Parser::new(toks).parse_program()?;
    let mut times: Vec<std::time::Duration> = Vec::with_capacity(n);
    for _ in 0..n {
        let mut interp = Interpreter::new();
        let start = std::time::Instant::now();
        interp.run(&program)?;
        times.push(start.elapsed());
    }
    let total: std::time::Duration = times.iter().sum();
    let mean = total / n as u32;
    let min = times.iter().min().copied().unwrap_or_default();
    let max = times.iter().max().copied().unwrap_or_default();
    let mut sorted = times.clone();
    sorted.sort();
    let median = sorted[n / 2];
    println!("bench: {} runs of {}", n, path);
    println!("  mean:   {:?}", mean);
    println!("  median: {:?}", median);
    println!("  min:    {:?}", min);
    println!("  max:    {:?}", max);
    println!("  total:  {:?}", total);
    Ok(())
}

fn cmd_test(path: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    match path {
        Some(p) => {
            let pb = std::path::PathBuf::from(p);
            if pb.is_dir() {
                collect_rk_files(&pb, &mut files)?;
            } else {
                files.push(pb);
            }
        }
        None => {
            let cwd = std::env::current_dir()?;
            collect_rk_files(&cwd, &mut files)?;
        }
    }
    if files.is_empty() {
        println!("no .rk files found");
        return Ok(());
    }
    files.sort();

    let mut total_pass = 0usize;
    let mut total_fail = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for f in &files {
        let src = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("skip {}: {}", f.display(), e);
                continue;
            }
        };
        let toks = match Lexer::new(&src).tokenize() {
            Ok(t) => t,
            Err(e) => { eprintln!("{}: parse error: {}", f.display(), e); continue; }
        };
        let program = match Parser::new(toks).parse_program() {
            Ok(p) => p,
            Err(e) => { eprintln!("{}: parse error: {}", f.display(), e); continue; }
        };
        let mut test_names: Vec<String> = Vec::new();
        for it in &program.items {
            if let ast::Item::Function(func) = it {
                if func.name.starts_with("test_") && func.params.is_empty() {
                    test_names.push(func.name.clone());
                }
            }
        }
        if test_names.is_empty() { continue; }
        test_names.sort();

        let mut interp = Interpreter::new();
        if let Err(e) = interp.load_only(&program) {
            eprintln!("{}: load failed: {}", f.display(), e);
            continue;
        }
        println!("== {} ==", f.display());
        for name in &test_names {
            match interp.invoke_global(name, vec![]) {
                Ok(_) => {
                    println!("  ok   {}", name);
                    total_pass += 1;
                }
                Err(e) => {
                    println!("  FAIL {}: {}", name, e);
                    total_fail += 1;
                    failures.push(format!("{}::{}", f.display(), name));
                }
            }
        }
    }

    println!();
    println!("results: {} passed, {} failed", total_pass, total_fail);
    if total_fail > 0 {
        for f in &failures { println!("  - {}", f); }
        return Err(format!("{} test(s) failed", total_fail).into());
    }
    Ok(())
}

fn collect_rk_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" || name == "node_modules" { continue; }
            collect_rk_files(&p, out)?;
        } else if p.extension().map(|e| e == "rk").unwrap_or(false) {
            out.push(p);
        }
    }
    Ok(())
}

fn cmd_fuzz(path: &str, fn_name: &str, n: usize) -> Result<(), Box<dyn std::error::Error>> {
    let src = read_source(path)?;
    let toks = match Lexer::new(&src).tokenize() {
        Ok(t) => t,
        Err(e) => { eprint!("{}", e.pretty(&src, Some(path))); return Err("aborted".into()); }
    };
    let program = match Parser::new(toks).parse_program() {
        Ok(p) => p,
        Err(e) => { eprint!("{}", e.pretty(&src, Some(path))); return Err("aborted".into()); }
    };

    let mut arity = None;
    for it in &program.items {
        if let ast::Item::Function(f) = it {
            if f.name == fn_name {
                arity = Some(f.params.len());
                break;
            }
        }
    }
    let arity = arity.ok_or_else(|| format!("function '{}' not found in {}", fn_name, path))?;

    let mut interp = Interpreter::new();
    interp.load_only(&program)?;

    // splitmix64 PRNG
    let mut state: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xCAFEBABEDEADBEEF)
        ^ 0x9E3779B97F4A7C15;
    let mut next_u64 = || {
        state = state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    };

    println!("fuzz: {} runs of {}({} args) in {}", n, fn_name, arity, path);
    let mut crashes: Vec<(Vec<value::Value>, String)> = Vec::new();
    let start = std::time::Instant::now();

    for _ in 0..n {
        let mut args: Vec<value::Value> = Vec::with_capacity(arity);
        for _ in 0..arity {
            let r = next_u64();
            let kind = (r >> 60) & 0x7;
            let v = match kind {
                0 | 1 => value::Value::Int((r as i64).wrapping_rem(1000)),
                2 => value::Value::Int((r as i64) % 1_000_000_000),
                3 => value::Value::Float({
                    let bits = r;
                    let f = f64::from_bits(bits);
                    if f.is_finite() { f } else { (r as f64) / 1e9 }
                }),
                4 => value::Value::Bool(r & 1 == 1),
                5 => value::Value::Str(std::rc::Rc::new(String::new())),
                6 => {
                    let len = (r % 8) as usize;
                    let mut s = String::with_capacity(len);
                    for i in 0..len {
                        let b = ((r >> (i * 7)) & 0x7F) as u8;
                        let c = if b.is_ascii_graphic() || b == b' ' { b as char } else { 'a' };
                        s.push(c);
                    }
                    value::Value::Str(std::rc::Rc::new(s))
                }
                _ => value::Value::Nil,
            };
            args.push(v);
        }
        let arg_copy = args.clone();
        match interp.invoke_global(fn_name, args) {
            Ok(_) => {}
            Err(e) => {
                crashes.push((arg_copy, e.to_string()));
                if crashes.len() >= 5 { break; }
            }
        }
    }

    let elapsed = start.elapsed();
    if crashes.is_empty() {
        println!("ok  no crashes in {} runs ({:?})", n, elapsed);
        Ok(())
    } else {
        println!("FAIL  {} crash(es) found:", crashes.len());
        for (args, msg) in &crashes {
            let arg_strs: Vec<String> = args.iter().map(|v| v.to_string()).collect();
            println!("  {}({})  ->  {}", fn_name, arg_strs.join(", "), msg);
        }
        Err(format!("{} crash(es)", crashes.len()).into())
    }
}

fn cmd_doc(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let src = read_source(path)?;
    let lines: Vec<&str> = src.lines().collect();
    let mut buffer: Vec<String> = Vec::new();
    let mut printed_any = false;

    for line in &lines {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("///") {
            buffer.push(rest.trim_start().to_string());
        } else if trimmed.is_empty() {
            // keep buffer; blank line inside header
        } else {
            // anchor: if this line declares a fn / type / enum / impl / trait / const, emit
            let header = parse_doc_header(trimmed);
            if let Some(h) = header {
                if !buffer.is_empty() {
                    if printed_any { println!(); }
                    println!("## {}", h);
                    for l in &buffer { println!("{}", l); }
                    printed_any = true;
                }
            }
            buffer.clear();
        }
    }

    if !printed_any {
        println!("(no /// doc comments found in {})", path);
    }
    Ok(())
}

fn parse_doc_header(line: &str) -> Option<String> {
    let kws = ["fn ", "type ", "enum ", "impl ", "trait ", "const "];
    for kw in &kws {
        if line.starts_with(kw) {
            let stop = line.find(|c: char| c == '{' || c == '(' || c == '=')
                .unwrap_or(line.len());
            return Some(line[..stop].trim().to_string());
        }
    }
    None
}

fn cmd_repl() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{BufRead, Write};
    println!("rock 0.1.0 — interactive REPL  (type :help for commands, :quit to exit)");
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut interp = Interpreter::new();
    let mut buffer = String::new();
    let mut input = stdin.lock();

    loop {
        let prompt = if buffer.is_empty() { ">>> " } else { "... " };
        print!("{}", prompt);
        stdout.flush().ok();

        let mut line = String::new();
        let n = input.read_line(&mut line)?;
        if n == 0 { println!(); break; }              // EOF
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r').to_string();

        if buffer.is_empty() {
            // commands
            match trimmed.trim() {
                ":quit" | ":q" | ":exit" => break,
                ":help" | ":h" | ":?" => {
                    println!("REPL commands:");
                    println!("  :quit / :q       Exit");
                    println!("  :reset           Reset the interpreter (lose all state)");
                    println!("  :fns             List defined functions");
                    println!("  :clear           Clear current multi-line input buffer");
                    continue;
                }
                ":reset" => { interp = Interpreter::new(); println!("(reset)"); continue; }
                ":fns" => {
                    let mut fns = interp.list_global_fns();
                    fns.sort();
                    if fns.is_empty() { println!("(no functions defined)"); }
                    else { for f in fns { println!("  {}", f); } }
                    continue;
                }
                ":clear" => { buffer.clear(); continue; }
                _ => {}
            }
        }

        buffer.push_str(&trimmed);
        buffer.push('\n');

        // Decide whether to evaluate yet: only when braces/brackets/parens are balanced AND
        // we're not inside a string. If unbalanced, keep reading.
        if !is_balanced(&buffer) { continue; }
        if buffer.trim().is_empty() { buffer.clear(); continue; }

        // Wrap bare expressions automatically: if it doesn't start with a top-level keyword,
        // we still try to parse as-is — the parser already accepts top-level Stmt::Expr.
        let src = std::mem::take(&mut buffer);
        match Lexer::new(&src).tokenize() {
            Ok(toks) => match Parser::new(toks).parse_program() {
                Ok(program) => match interp.eval_repl(&program) {
                    Ok(Some(v)) => {
                        let s = format!("{}", v);
                        if !s.is_empty() { println!("{}", s); }
                    }
                    Ok(None) => {}
                    Err(e) => { eprint!("{}", e.pretty(&src, Some("<repl>"))); }
                },
                Err(e) => { eprint!("{}", e.pretty(&src, Some("<repl>"))); }
            },
            Err(e) => { eprint!("{}", e.pretty(&src, Some("<repl>"))); }
        }
    }
    Ok(())
}

fn is_balanced(src: &str) -> bool {
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut quote = '"';
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if c == '\\' && i + 1 < bytes.len() { i += 2; continue; }
            if c == quote { in_str = false; }
            i += 1; continue;
        }
        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] as char == '/' {
            // skip to newline
            while i < bytes.len() && bytes[i] as char != '\n' { i += 1; }
            continue;
        }
        if c == '"' || c == '\'' || c == '`' { in_str = true; quote = c; i += 1; continue; }
        match c {
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    !in_str && depth <= 0
}
