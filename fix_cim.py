import re

with open('scripts/windows/Run-GuestExhaustive.ps1', 'r') as f:
    text = f.read()

replacement = r'''$( $wqlParam = "ramshared"; if ($wqlParam -match "['\\]") { throw "invalid WQL parameter" }; Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = '$($wqlParam -replace ""'"", ""''"")'" -ErrorAction Stop )'''

text = text.replace(
    'Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = \'ramshared\'" -ErrorAction Stop',
    replacement
)

with open('scripts/windows/Run-GuestExhaustive.ps1', 'w') as f:
    f.write(text)
