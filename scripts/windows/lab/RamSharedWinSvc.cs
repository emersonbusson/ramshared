// Lab SCM service: orchestrates Start/Stop-RamSharedLab.ps1 (DT-9).
// Build (guest/admin):
//   csc /nologo /target:exe /platform:x64 /r:System.ServiceProcess.dll ^
//       /out:C:\ramshared\bin\RamSharedWinSvc.exe RamSharedWinSvc.cs
// Install:
//   sc create RamSharedWinSvc binPath= "C:\ramshared\bin\RamSharedWinSvc.exe" start= delayed-auto
//   sc start RamSharedWinSvc
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
                StartPsAsync(StartPs1, "-FormatIfNeeded");
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
            StartPsAsync(StartPs1, "-FormatIfNeeded");
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
            int code = RunPsWait(StopPs1, "-Drive D", 60000);
            Log("OnStop exit=" + code);
            // exit 2 = refuse kill (pagefile hot)
            if (code == 2)
                Log("DT9_REFUSE pagefile still hot - backend not killed");
        }
        catch (Exception ex)
        {
            Log("OnStop FAIL " + ex);
        }
    }

    protected override void OnShutdown()
    {
        OnStop();
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
