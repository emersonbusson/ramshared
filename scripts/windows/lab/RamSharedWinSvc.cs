// Lab SCM service: orchestrates Start/Stop-RamSharedLab.ps1 (DT-9).
// Build (guest/admin):
//   csc /nologo /target:exe /platform:x64 /r:System.ServiceProcess.dll ^
//       /out:C:\ramshared\bin\RamSharedWinSvc.exe RamSharedWinSvc.cs
// Install:
//   .\scripts\windows\Install-RamSharedService.ps1 -RepoRoot <repo> -StartNow
//
// DT-9: OnStop throws if Stop-RamSharedLab exits 2 (pagefile still hot) so SCM
// does not mark the service Stopped while the backend must stay alive.
using System;
using System.Diagnostics;
using System.IO;
using System.ServiceProcess;

public class RamSharedWinSvc : ServiceBase
{
    const string StartPs1 = @"C:\ramshared\bin\Start-RamSharedLab.ps1";
    const string StopPs1 = @"C:\ramshared\bin\Stop-RamSharedLab.ps1";
    const string LogPath = @"C:\ramshared\bin\winsvc.log";

    public RamSharedWinSvc()
    {
        ServiceName = "RamSharedWinSvc";
        CanStop = true;
        CanShutdown = true;
        AutoLog = true;
    }

    static void Main(string[] args)
    {
        if (args != null && args.Length > 0)
        {
            string a = args[0].ToLowerInvariant();
            if (a == "console" || a == "run")
            {
                StartPsAsync(StartPs1, BuildStartArgs());
                Console.WriteLine("console start spawned; use stop-console for DT-9 stop");
                return;
            }
            if (a == "stop-console")
            {
                int c = RunPsWait(StopPs1, "-Drive D", 60000);
                Environment.Exit(c);
                return;
            }
        }
        ServiceBase.Run(new RamSharedWinSvc());
    }

    protected override void OnStart(string[] args)
    {
        // SCM requires OnStart to return quickly. Spawn lab start asynchronously.
        try
        {
            Log("OnStart async");
            StartPsAsync(StartPs1, BuildStartArgs());
            Log("OnStart spawned");
        }
        catch (Exception ex)
        {
            Log("OnStart FAIL " + ex);
            throw;
        }
    }

    protected override void OnStop()
    {
        try
        {
            Log("OnStop DT-9");
            int code = RunPsWait(StopPs1, "-Drive D", 120000);
            Log("OnStop exit=" + code);
            // exit 2 = refuse kill (pagefile hot) - fail closed for SCM (#29).
            if (code == 2)
            {
                Log("DT9_REFUSE pagefile still hot - backend not killed; abort service stop");
                throw new InvalidOperationException(
                    "DT-9: secondary pagefile still allocated; refuse service stop (BugCheck 0x7A risk)");
            }
            if (code != 0)
            {
                Log("OnStop non-zero " + code);
                throw new InvalidOperationException("Stop-RamSharedLab failed exit=" + code);
            }
        }
        catch (InvalidOperationException)
        {
            throw;
        }
        catch (Exception ex)
        {
            Log("OnStop FAIL " + ex);
            throw;
        }
    }

    protected override void OnShutdown()
    {
        // Best-effort ordered stop; do not swallow DT-9 refuse.
        OnStop();
    }

    static string BuildStartArgs()
    {
        // Machine env set by Install-RamSharedService.ps1 -ForceFormat
        string force = Environment.GetEnvironmentVariable(
            "RAMSHARED_WINSVC_FORCE_FORMAT", EnvironmentVariableTarget.Machine);
        if (string.IsNullOrEmpty(force))
            force = Environment.GetEnvironmentVariable("RAMSHARED_WINSVC_FORCE_FORMAT");
        if (!string.IsNullOrEmpty(force) &&
            (force == "1" || force.Equals("true", StringComparison.OrdinalIgnoreCase)))
            return "-FormatIfNeeded -Force";
        // Safe default under SCM: start backend, do not Clear-Disk without Force.
        return "";
    }

    static void StartPsAsync(string script, string extraArgs)
    {
        if (!File.Exists(script))
        {
            Log("missing " + script);
            return;
        }
        var psi = new ProcessStartInfo
        {
            FileName = "powershell.exe",
            Arguments = "-NoProfile -ExecutionPolicy Bypass -File \"" + script + "\" " + extraArgs,
            UseShellExecute = false,
            CreateNoWindow = true
        };
        Process.Start(psi);
    }

    static int RunPsWait(string script, string extraArgs, int timeoutMs)
    {
        if (!File.Exists(script))
        {
            Log("missing " + script);
            return 1;
        }
        var psi = new ProcessStartInfo
        {
            FileName = "powershell.exe",
            Arguments = "-NoProfile -ExecutionPolicy Bypass -File \"" + script + "\" " + extraArgs,
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true
        };
        using (var p = Process.Start(psi))
        {
            if (!p.WaitForExit(timeoutMs))
            {
                try { p.Kill(); } catch { }
                Log("ps timeout");
                return 1;
            }
            string so = p.StandardOutput.ReadToEnd();
            string se = p.StandardError.ReadToEnd();
            Log("ps out=" + so.Replace("\r", " ").Replace("\n", " "));
            if (!string.IsNullOrEmpty(se))
                Log("ps err=" + se.Replace("\r", " ").Replace("\n", " "));
            return p.ExitCode;
        }
    }

    static void Log(string msg)
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(LogPath));
            File.AppendAllText(LogPath, DateTime.Now.ToString("o") + " " + msg + "\r\n");
        }
        catch { }
    }
}
