import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/network_service.dart';

/// Network settings — relay management (add/remove), connection status, and
/// transport toggles. Port of the legacy network settings section.
class NetworkSettingsScreen extends StatefulWidget {
  const NetworkSettingsScreen({super.key});

  @override
  State<NetworkSettingsScreen> createState() => _NetworkSettingsScreenState();
}

class _NetworkSettingsScreenState extends State<NetworkSettingsScreen> {
  final TextEditingController _relayField = TextEditingController();
  bool _loading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    final service = context.read<NetworkService>();
    try {
      await service.refresh();
      await service.fetchRelayStatus();
      _error = null;
    } catch (e) {
      _error = '$e';
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _addRelay() async {
    final url = _relayField.text.trim();
    if (url.isEmpty) return;
    final service = context.read<NetworkService>();
    final ok = await service.addRelay(url);
    if (!mounted) return;
    if (ok) {
      _relayField.clear();
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Connected to $url')),
      );
      _refresh();
    } else {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Could not connect to that relay')),
      );
    }
  }

  Future<void> _removeRelay(String url) async {
    final service = context.read<NetworkService>();
    await service.removeRelay(url);
    _refresh();
  }

  Future<void> _setTransport(bool on) async {
    final service = context.read<NetworkService>();
    final mode = on ? TransportMode.i2p : TransportMode.clearnet;
    try {
      final ok = await service.setTransportMode(mode);
      if (!mounted) return;
      if (ok) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Transport: ${mode.name}')),
        );
        await service.reinitRelays();
        _refresh();
      } else {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Could not switch transport')),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Transport switch failed: $e')),
        );
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final service = context.watch<NetworkService>();
    return Scaffold(
      appBar: AppBar(title: const Text('Network')),
      body: RefreshIndicator(
        onRefresh: _refresh,
        child: ListView(
          padding: const EdgeInsets.all(16),
          children: [
            Text('Transports', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 4),
            ListTile(
              leading: Icon(
                service.i2p == true
                    ? Icons.check_circle
                    : Icons.circle_outlined,
                color: service.i2p == true ? Colors.green : null,
              ),
              title: const Text('I2P tunnel (local i2pd SOCKS 7656)'),
            ),
            ListTile(
              leading: Icon(
                service.freenet == true
                    ? Icons.check_circle
                    : Icons.circle_outlined,
                color: service.freenet == true ? Colors.green : null,
              ),
              title: const Text('Freenet gateway (local port 8888)'),
            ),
            const Divider(),
            Text('Relays', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 4),
            Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: _relayField,
                    decoration: const InputDecoration(
                      hintText: 'wss://relay.example.com',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                    onSubmitted: (_) => _addRelay(),
                  ),
                ),
                const SizedBox(width: 8),
                FilledButton(
                  onPressed: _addRelay,
                  child: const Text('Add'),
                ),
              ],
            ),
            const SizedBox(height: 8),
            if (_error != null)
              Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: Text(
                  'Error: $_error',
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
              ),
            if (_loading)
              const Padding(
                padding: EdgeInsets.all(24),
                child: Center(child: CircularProgressIndicator()),
              )
            else
              for (final relay in service.relays)
                ListTile(
                  dense: true,
                  leading: Icon(
                    relay.connected ? Icons.wifi : Icons.wifi_off,
                    color: relay.connected ? Colors.green : null,
                  ),
                  title: Text(
                    relay.url,
                    overflow: TextOverflow.ellipsis,
                  ),
                  subtitle: Text(
                    relay.connected
                        ? '${relay.latencyMs} ms · last event ${_ago(relay.lastEventAt)}'
                        : 'disconnected',
                  ),
                  trailing: IconButton(
                    icon: const Icon(Icons.delete_outline),
                    tooltip: 'Remove relay',
                    onPressed: () => _removeRelay(relay.url),
                  ),
                ),
          ],
        ),
      ),
    );
  }

  String _ago(int ts) {
    if (ts <= 0) return 'never';
    final d = DateTime.now()
        .difference(DateTime.fromMillisecondsSinceEpoch(ts * 1000));
    if (d.inMinutes < 1) return 'just now';
    if (d.inHours < 1) return '${d.inMinutes}m ago';
    return '${d.inHours}h ago';
  }
}
