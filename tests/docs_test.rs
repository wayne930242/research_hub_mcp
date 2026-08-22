// Tests for documentation consistency and completeness
// This ensures all documentation files are valid and consistent

use std::fs;
use std::path::Path;

#[test]
fn test_all_docs_exist() {
    let docs_dir = "docs";
    let required_docs = [
        "USER_GUIDE.md",
        "TROUBLESHOOTING.md",
        "ARCHITECTURE.md",
        "SECURITY.md",
        "HOMEBREW.md",
        "LAUNCHAGENT.md",
    ];

    for doc in &required_docs {
        let path = Path::new(docs_dir).join(doc);
        assert!(
            path.exists(),
            "Required documentation file {doc} does not exist"
        );

        // Check file is not empty
        let content = fs::read_to_string(&path).expect("Failed to read doc file");
        assert!(
            !content.trim().is_empty(),
            "Documentation file {doc} is empty"
        );
        assert!(content.len() > 100, "Documentation file {doc} is too short");
    }
}

#[test]
fn test_port_consistency() {
    let docs_dir = "docs";
    let doc_files = [
        "USER_GUIDE.md",
        "TROUBLESHOOTING.md",
        "HOMEBREW.md",
        "LAUNCHAGENT.md",
    ];

    for doc_file in &doc_files {
        let path = Path::new(docs_dir).join(doc_file);
        let content = fs::read_to_string(&path).expect("Failed to read doc file");

        // Check for port 8080 consistency (should not have 8090)
        assert!(
            !content.contains("8090"),
            "File {doc_file} contains inconsistent port 8090, should use 8080"
        );
    }
}

#[test]
fn test_security_doc_structure() {
    let security_doc = Path::new("docs").join("SECURITY.md");
    let content = fs::read_to_string(&security_doc).expect("Failed to read SECURITY.md");

    // Check for required sections
    let required_sections = [
        "# Security Considerations",
        "## Security Model",
        "## Threat Analysis",
        "## Security Controls",
        "## Network Security",
        "## File System Security",
        "## Operational Security",
    ];

    for section in &required_sections {
        assert!(
            content.contains(section),
            "SECURITY.md missing required section: {section}"
        );
    }
}

#[test]
fn test_user_guide_structure() {
    let user_guide = Path::new("docs").join("USER_GUIDE.md");
    let content = fs::read_to_string(&user_guide).expect("Failed to read USER_GUIDE.md");

    // Check for required sections
    let required_sections = [
        "# User Guide",
        "## Getting Started",
        "## Common Workflows",
        "## Configuration",
        "## Integration with Claude Desktop",
        "## Usage Examples",
        "## Best Practices",
    ];

    for section in &required_sections {
        assert!(
            content.contains(section),
            "USER_GUIDE.md missing required section: {section}"
        );
    }
}

#[test]
fn test_troubleshooting_structure() {
    let troubleshooting = Path::new("docs").join("TROUBLESHOOTING.md");
    let content = fs::read_to_string(&troubleshooting).expect("Failed to read TROUBLESHOOTING.md");

    // Check for required sections
    let required_sections = [
        "# Troubleshooting Guide",
        "## Quick Diagnostics",
        "## Installation Issues",
        "## Service Startup Issues",
        "## Network and Connectivity Issues",
        "## Configuration Issues",
        "## Performance Issues",
    ];

    for section in &required_sections {
        assert!(
            content.contains(section),
            "TROUBLESHOOTING.md missing required section: {section}"
        );
    }
}

#[test]
fn test_architecture_structure() {
    let architecture = Path::new("docs").join("ARCHITECTURE.md");
    let content = fs::read_to_string(&architecture).expect("Failed to read ARCHITECTURE.md");

    // Check for required sections
    let required_sections = [
        "# Architecture Documentation",
        "## High-Level Architecture",
        "## Component Overview",
        "## Data Flow",
        "## Design Patterns",
        "## Technology Choices",
        "## Security Architecture",
    ];

    for section in &required_sections {
        assert!(
            content.contains(section),
            "ARCHITECTURE.md missing required section: {section}"
        );
    }
}

#[test]
fn test_no_broken_internal_links() {
    let docs_dir = "docs";
    let doc_files = [
        "USER_GUIDE.md",
        "TROUBLESHOOTING.md",
        "ARCHITECTURE.md",
        "SECURITY.md",
        "HOMEBREW.md",
        "LAUNCHAGENT.md",
    ];

    for doc_file in &doc_files {
        let path = Path::new(docs_dir).join(doc_file);
        let content = fs::read_to_string(&path).expect("Failed to read doc file");

        // Check for common markdown link patterns that reference other docs
        if content.contains("](TROUBLESHOOTING.md)") {
            let troubleshooting_path = Path::new(docs_dir).join("TROUBLESHOOTING.md");
            assert!(
                troubleshooting_path.exists(),
                "File {doc_file} links to TROUBLESHOOTING.md but it doesn't exist"
            );
        }

        if content.contains("](README.md)") {
            let readme_path = Path::new("README.md");
            assert!(
                readme_path.exists(),
                "File {doc_file} links to README.md but it doesn't exist"
            );
        }
    }
}

#[test]
fn test_code_examples_are_marked() {
    let security_doc = Path::new("docs").join("SECURITY.md");
    let content = fs::read_to_string(&security_doc).expect("Failed to read SECURITY.md");

    // Security doc should mark code examples as examples to avoid confusion
    assert!(
        content.contains("// Example implementation"),
        "SECURITY.md should mark code examples clearly"
    );
}

#[test]
fn test_toc_sections_exist() {
    let docs_dir = "docs";
    let doc_files = [
        "USER_GUIDE.md",
        "TROUBLESHOOTING.md",
        "ARCHITECTURE.md",
        "SECURITY.md",
    ];

    for doc_file in &doc_files {
        let path = Path::new(docs_dir).join(doc_file);
        let content = fs::read_to_string(&path).expect("Failed to read doc file");

        // Each doc should have a table of contents
        assert!(
            content.contains("## Table of Contents") || content.contains("# Table of Contents"),
            "File {doc_file} should have a Table of Contents section"
        );
    }
}

#[test]
fn test_configuration_examples_valid_toml() {
    let user_guide = Path::new("docs").join("USER_GUIDE.md");
    let content = fs::read_to_string(&user_guide).expect("Failed to read USER_GUIDE.md");

    // Extract TOML blocks and validate they parse
    let mut in_toml_block = false;
    let mut toml_content = String::new();

    for line in content.lines() {
        if line.trim() == "```toml" {
            in_toml_block = true;
            toml_content.clear();
        } else if line.trim() == "```" && in_toml_block {
            in_toml_block = false;
            // Validate this TOML block
            if !toml_content.trim().is_empty() {
                match toml::from_str::<toml::Value>(&toml_content) {
                    Ok(_) => {} // Valid TOML
                    Err(e) => panic!("Invalid TOML in USER_GUIDE.md: {e}"),
                }
            }
        } else if in_toml_block {
            toml_content.push_str(line);
            toml_content.push('\n');
        }
    }
}
