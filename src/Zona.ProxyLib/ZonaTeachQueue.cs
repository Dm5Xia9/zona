using System.Threading.Channels;

namespace Zona.ProxyLib;

public sealed class ZonaTeachQueue : IZonaTeachQueue
{
    private readonly Channel<ZonaTeachSignal> _channel = Channel.CreateUnbounded<ZonaTeachSignal>(new UnboundedChannelOptions
    {
        SingleReader = true,
        SingleWriter = false,
        AllowSynchronousContinuations = false,
    });

    public ChannelReader<ZonaTeachSignal> Reader => _channel.Reader;

    public bool TryEnqueue(ZonaTeachSignal signal) => _channel.Writer.TryWrite(signal);
}
