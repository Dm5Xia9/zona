using System.Threading.Channels;

namespace Zona.ProxyLib;

public interface IZonaTeachQueue
{
    bool TryEnqueue(ZonaTeachSignal signal);

    ChannelReader<ZonaTeachSignal> Reader { get; }
}
