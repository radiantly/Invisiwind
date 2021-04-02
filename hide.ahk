; If executable is built locally, use that instead
if FileExist(".\Build\bin\Release\Invisiwind.exe")
    SetWorkingDir Build\bin\Release

; When CTRL + J is pressed
^j::
WinGet, pid, PID, A
Run Invisiwind.exe --hide %pid%,, Hide
return
