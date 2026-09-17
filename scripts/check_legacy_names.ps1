$ErrorActionPreference = 'Stop'

& rg -n -i --glob '!target/**' --glob '!graphify-out/**' --glob '!.git/**' --glob '!.agents/**' --glob '!CONTRIBUTING.md' --glob '!scripts/check_legacy_names.ps1' 'PowerLeaf|Smart Saver|Smart Trim|Background CPU Restriction|Core Steering|Soft CPU Sets|Hard CPU Affinity|background_cpu_restriction|core_steering|soft_cpu_sets|hard_cpu_affinity|serde.*alias|fill_missing_power_plan_mappings|Settings::power_plans' .
if ($LASTEXITCODE -eq 0) { throw 'Legacy names or compatibility aliases found.' }
if ($LASTEXITCODE -ne 1) { throw "Legacy-name search failed with exit code $LASTEXITCODE." }
Write-Host 'Legacy-name check passed.'
exit 0
