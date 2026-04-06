namespace Zona.ProxyLib;

internal static class ZonaExecutableResolver
{
    internal static string Resolve(ZonaOptions options)
    {
        if (!string.IsNullOrWhiteSpace(options.ExecutablePath))
        {
            var p = options.ExecutablePath.Trim();
            return Path.IsPathRooted(p) ? p : Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, p));
        }

        var env = Environment.GetEnvironmentVariable("ZONA_EXECUTABLE");
        if (!string.IsNullOrWhiteSpace(env))
            return Path.IsPathRooted(env) ? env : Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, env.Trim()));

        return Path.Combine(AppContext.BaseDirectory, OperatingSystem.IsWindows() ? "zona.exe" : "zona");
    }
}
