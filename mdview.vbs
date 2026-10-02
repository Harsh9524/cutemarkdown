' Silent launcher: no console flash. Usage: wscript mdview.vbs "file.md"
Set fso = CreateObject("Scripting.FileSystemObject")
dir = fso.GetParentFolderName(WScript.ScriptFullName)
arg = ""
If WScript.Arguments.Count > 0 Then arg = " -Path """ & WScript.Arguments(0) & """"
CreateObject("WScript.Shell").Run "powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File """ & dir & "\mdview.ps1""" & arg, 0, False
