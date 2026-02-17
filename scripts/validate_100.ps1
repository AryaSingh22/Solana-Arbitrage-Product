$ErrorActionPreference = "Stop"

Write-Host "Starting Final Validation Suite..." -ForegroundColor Cyan

# 1. Check Build
Write-Host "Checking Build..." -ForegroundColor Yellow
cargo check --workspace
if ($LASTEXITCODE -ne 0) { throw "Build failed!" }

# 2. Run Tests
Write-Host "Running Workspace Tests..." -ForegroundColor Yellow
cargo test --workspace
if ($LASTEXITCODE -ne 0) { throw "Tests failed!" }

# 3. Clippy Linting
Write-Host "Running Clippy..." -ForegroundColor Yellow
cargo clippy --workspace -- -D warnings
if ($LASTEXITCODE -ne 0) { throw "Clippy check failed!" }

# 4. Benchmarks (Core only)
Write-Host "Running Benchmarks (Core)..." -ForegroundColor Yellow
cargo bench -p solana-arb-core
if ($LASTEXITCODE -ne 0) { throw "Benchmarks failed!" }

Write-Host "ALL CHECKS PASSED! Project is 100/100 Ready." -ForegroundColor Green
