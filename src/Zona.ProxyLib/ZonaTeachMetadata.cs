using Microsoft.AspNetCore.Builder;

namespace Zona.ProxyLib;

/// <summary>Метаданные эндпоинта: не отправлять запрос в teach Zona.</summary>
public interface ISuppressZonaTeachMetadata { }

/// <summary>Маркер для <see cref="RouteHandlerBuilder.WithMetadata"/>.</summary>
public sealed class SuppressZonaTeachMetadata : ISuppressZonaTeachMetadata;

public static class ZonaTeachMetadataExtensions
{
    /// <summary>Исключить эндпоинт из обучения Zona (health, dev-диагностика и т.д.).</summary>
    public static RouteHandlerBuilder SuppressZonaTeach(this RouteHandlerBuilder builder) =>
        builder.WithMetadata(new SuppressZonaTeachMetadata());
}
