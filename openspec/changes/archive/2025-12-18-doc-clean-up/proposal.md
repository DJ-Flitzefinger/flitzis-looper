# Documentation Clean-up and Quality Initiative

## Change ID
`doc-clean-up`

## Summary
This proposal outlines a comprehensive plan to streamline and modernize the flitzis-looper documentation. The goal is to eliminate redundancy, remove false or outdated information, consolidate duplicate content, and establish a clear hierarchy of documentation that reflects the current state of the project while preserving valuable research for future evolution.

## Objectives
- Remove all version numbers from documentation (source of truth: pyproject.toml)
- Eliminate references to non-existent dependencies (liblo)
- Remove all mentions of "BMAD documentation" and BMAD-specific information
- Correct false architectural claims (monolithic vs modular)
- Consolidate redundant information across documents
- Standardize command examples (use uv exclusively)
- Create a single authoritative source for each type of information
- Radically reduce documentation volume while preserving essential information
- Maintain research documents as separate future-evolution proposals

## Key Changes

### 1. Document Consolidation
- **Remove**: index.md, project-overview.md, comprehensive-analysis.md
- **Keep and enhance**: architecture.md, source-tree-analysis.md, technology-stack.md, development-guide.md, optimizations.md
- **Keep as-is**: research/ (off-limits per instructions), todos.md

### 2. Content Removals
- Remove all version numbers from technology stack
- Remove "liblo" dependency references
- Remove "BMAD documentation" mentions
- Remove "Monolith with 1 part" claims
- Remove redundant architecture pattern descriptions
- Remove redundant source tree descriptions
- Remove redundant entry point descriptions
- Remove redundant development commands

### 3. Content Consolidations
- Move all dependency information to pyproject.toml (single source of truth)
- Move all development commands to development-guide.md (single source)
- Move all architecture details to architecture.md (single source)
- Move all source tree details to source-tree-analysis.md (single source)
- Move all performance optimization details to optimizations.md (single source)

### 4. Documentation Hierarchy

```
docs/
├── README.md                 # Project overview and usage
├── architecture.md           # System architecture and design
├── source-tree-analysis.md   # Codebase structure and component mapping
├── technology-stack.md       # Technologies used (without versions)
├── development-guide.md      # Setup, commands, and workflow
├── optimizations.md          # Performance improvements and strategies
├── todos.md                  # TODOS (unchanged: should be migrated to openspec proposals, but ignored for this taskke)
└── research/                 # Future evolution proposals (unchanged)
```

### 5. Content Standards
- Use concise bullet points only
- Remove all excessive rambling and "AI slop"
- Use consistent terminology (lowercase "component-based desktop application")
- Use uv exclusively for all commands
- Reference pyproject.toml as the source of truth for dependencies
- Use absolute paths in all code examples
- Add cross-references between documents

## Validation
- Ensure all documentation references pyproject.toml for dependency versions
- Ensure no references to liblo remain
- Ensure no BMAD terminology remains
- Ensure all architecture claims match current implementation
- Ensure all commands use uv
- Ensure documentation is 70% smaller than current state

## Dependencies
- None (documentation-only change)

## Risks
- Low: This is a documentation-only change that doesn't affect code
- Medium: Risk of accidentally removing valuable information

## Benefits
- Reduced maintenance burden
- Improved clarity for new developers
- Eliminated confusion from conflicting information
- Faster onboarding
- Professional appearance

## Next Steps
1. Implement this proposal
2. Submit for review
