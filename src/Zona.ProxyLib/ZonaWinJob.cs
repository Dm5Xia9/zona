using System.Diagnostics;
using System.Runtime.InteropServices;
using Microsoft.Extensions.Logging;

namespace Zona.ProxyLib;

/// <summary>
/// Windows Job Object: при завершении родителя ОС закрывает job — дочерний процесс останавливается.
/// </summary>
internal static class ZonaWinJob
{
    private const uint JobObjectExtendedLimitInformation = 9;
    private const uint JobObjectLimitKillOnJobClose = 0x2000;

    internal static nint TryCreateAndAssign(Process process, ILogger logger)
    {
        if (!OperatingSystem.IsWindows())
            return 0;

        nint job = CreateJobObject(0, 0);
        if (job == 0)
        {
            logger.LogWarning("Zona: CreateJobObject failed ({Code})", Marshal.GetLastPInvokeError());
            return 0;
        }

        var info = new JobobjectExtendedLimitInformation
        {
            BasicLimitInformation = new JobobjectBasicLimitInformation { LimitFlags = JobObjectLimitKillOnJobClose },
        };

        var len = (uint)Marshal.SizeOf<JobobjectExtendedLimitInformation>();
        if (!SetInformationJobObject(job, JobObjectExtendedLimitInformation, ref info, len))
        {
            logger.LogWarning("Zona: SetInformationJobObject failed ({Code})", Marshal.GetLastPInvokeError());
            CloseHandle(job);
            return 0;
        }

        if (!AssignProcessToJobObject(job, process.Handle))
        {
            logger.LogWarning("Zona: AssignProcessToJobObject failed ({Code})", Marshal.GetLastPInvokeError());
            CloseHandle(job);
            return 0;
        }

        logger.LogInformation("Zona: child process assigned to Windows job (kill on parent exit)");
        return job;
    }

    internal static void CloseJob(nint job)
    {
        if (job != 0)
            CloseHandle(job);
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern nint CreateJobObjectW(nint lpJobAttributes, nint lpName);

    private static nint CreateJobObject(nint a, nint b) => CreateJobObjectW(a, b);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetInformationJobObject(nint hJob, uint jobObjectInfoClass, ref JobobjectExtendedLimitInformation jobObjectInfo, uint cbJobObjectInfoLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool AssignProcessToJobObject(nint hJob, nint hProcess);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CloseHandle(nint hObject);

    [StructLayout(LayoutKind.Sequential)]
    private struct IoCounters
    {
        public ulong ReadOperationCount;
        public ulong WriteOperationCount;
        public ulong OtherOperationCount;
        public ulong ReadTransferCount;
        public ulong WriteTransferCount;
        public ulong OtherTransferCount;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct JobobjectBasicLimitInformation
    {
        public long PerProcessUserTimeLimit;
        public long PerJobUserTimeLimit;
        public uint LimitFlags;
        public nuint MinimumWorkingSetSize;
        public nuint MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public nuint Affinity;
        public uint PriorityClass;
        public uint SchedulingClass;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct JobobjectExtendedLimitInformation
    {
        public JobobjectBasicLimitInformation BasicLimitInformation;
        public IoCounters IoInfo;
        public nuint ProcessMemoryLimit;
        public nuint JobMemoryLimit;
        public nuint PeakProcessMemoryUsed;
        public nuint PeakJobMemoryUsed;
    }
}
