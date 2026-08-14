import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../services/mesh_service.dart';
import '../services/network_service.dart';
import '../services/session_service.dart';
import '../widgets/error_state_text.dart';

/// Network: relay connections, anonymity transports, and diagnostics.
class NetworkScreen extends StatefulWidget {
  /// Network screen.
  const NetworkScreen({super.key});

  @override
  State<NetworkScreen> createState() => _NetworkScreenState();
}

class _NetworkScreenState extends State<NetworkScreen> {
  final TextEditingController _relayUrl = TextEditingController();
  bool _loadingRelays = false;
  bool _loadingDiagnostics = false;
  bool _loadingBearers = false;
  String? _relayError;
  String _diagnostics = '';
  Map<String, dynamic> _bearers = {};
  bool _transportsLoaded = false;
  bool _startingMesh = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _loadRelays());
  }

  @override
  void dispose() {
    _relayUrl.dispose();
    super.dispose();
  }

  Future<void> _loadRelays() async {
    setState(() => _loadingRelays = true);
    try {
      await context.read<NetworkService>().fetchRelayStatus();
      _relayError = null;
    } catch (e) {
      debugPrint('relay status: $e');
      _relayError = e.toString();
    }
    if (mounted) setState(() => _loadingRelays = false);
  }

  Future<void> _addRelay() async {
    final url = _relayUrl.text.trim();
    if (url.isEmpty) return;
    try {
      final ok = await context.read<NetworkService>().addRelay(url);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(ok ? 'Relay added' : 'Could not add relay')),
        );
        if (ok) _relayUrl.clear();
      }
    } catch (e) {
      debugPrint('add relay: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Add relay: $e')));
      }
    }
    await _loadRelays();
  }

  Future<void> _removeRelay(String url) async {
    try {
      await context.read<NetworkService>().removeRelay(url);
    } catch (e) {
      debugPrint('remove relay: $e');
    }
    await _loadRelays();
  }

  Future<void> _initFromAccountRelays() async {
    final relays = context.read<SessionService>().activeAccount?.relayList;
    if (relays == null || relays.isEmpty) return;
    try {
      await context.read<NetworkService>().initRelays(relays);
    } catch (e) {
      debugPrint('init relays: $e');
    }
    await _loadRelays();
  }

  Future<void> _loadTransports() async {
    setState(() {
      _loadingBearers = true;
      _transportsLoaded = true;
    });
    final network = context.read<NetworkService>();
    await network.refresh();
    if (!mounted) return;
    final ownPubkey = context.read<SessionService>().activePubkey;
    if (ownPubkey != null) {
      try {
        final bearers = await network.fetchMultiBearerStatus(ownPubkey);
        if (mounted) setState(() => _bearers = bearers);
      } catch (e) {
        debugPrint('multi-bearer: $e');
      }
    }
    if (mounted) setState(() => _loadingBearers = false);
  }

  Future<void> _loadDiagnostics() async {
    setState(() => _loadingDiagnostics = true);
    try {
      final json = context.read<NetworkService>().fetchSysDiagnostics();
      if (mounted) {
        setState(() {
          _diagnostics =
              const JsonEncoder.withIndent('  ').convert(jsonDecode(json));
        });
      }
    } catch (e) {
      debugPrint('sys diagnostics: $e');
    }
    if (mounted) setState(() => _loadingDiagnostics = false);
  }

  Future<void> _startMesh() async {
    final mesh = context.read<MeshService>();
    if (mesh.running) return;
    setState(() => _startingMesh = true);
    try {
      await mesh.startTransport();
    } catch (e) {
      debugPrint('start mesh: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Start mesh: $e')));
      }
    }
    if (mounted) setState(() => _startingMesh = false);
  }

  Future<void> _refreshMesh() async {
    try {
      await context.read<MeshService>().refreshStatus();
    } catch (e) {
      debugPrint('refresh mesh: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Refresh mesh: $e')));
      }
    }
  }

  Future<void> _announceMesh() async {
    try {
      await context.read<MeshService>().announce();
    } catch (e) {
      debugPrint('announce mesh: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Announce: $e')));
      }
    }
  }

  String _shortHash(String? hash) {
    if (hash == null || hash.isEmpty) return '—';
    return hash.length <= 16 ? hash : '${hash.substring(0, 16)}…';
  }

  @override
  Widget build(BuildContext context) {
    return DefaultTabController(
      length: 3,
      child: Scaffold(
        appBar: AppBar(
          title: const Text('Network'),
          bottom: const TabBar(
            tabs: [
              Tab(text: 'Relays'),
              Tab(text: 'Transports'),
              Tab(text: 'Diagnostics'),
            ],
          ),
        ),
        body: TabBarView(
          children: [_relaysTab(), _transportsTab(), _diagnosticsTab()],
        ),
      ),
    );
  }

  Widget _relaysTab() {
    final theme = Theme.of(context);
    return RefreshIndicator(
      onRefresh: _loadRelays,
      child: Consumer<NetworkService>(
        builder: (context, network, _) {
          return ListView(
            physics: const AlwaysScrollableScrollPhysics(),
            padding: const EdgeInsets.symmetric(vertical: 8),
            children: [
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: Row(
                  children: [
                    Expanded(
                      child: TextField(
                        controller: _relayUrl,
                        decoration: const InputDecoration(
                          hintText: 'wss://relay.example.com',
                          border: OutlineInputBorder(),
                          isDense: true,
                        ),
                        textInputAction: TextInputAction.done,
                        onSubmitted: (_) => _addRelay(),
                      ),
                    ),
                    const SizedBox(width: 8),
                    IconButton(
                      icon: const Icon(Icons.add_circle_outline),
                      tooltip: 'Add relay',
                      onPressed: _addRelay,
                    ),
                  ],
                ),
              ),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: Text(
                  'Relay Connections',
                  style: theme.textTheme.titleMedium,
                ),
              ),
              TextButton.icon(
                onPressed: _initFromAccountRelays,
                icon: const Icon(Icons.refresh),
                label: const Text('Reconnect from account relays'),
              ),
              if (_loadingRelays)
                const Padding(
                  padding: EdgeInsets.all(16),
                  child: Center(child: CircularProgressIndicator()),
                )
              else if (_relayError != null)
                Padding(
                  padding: const EdgeInsets.all(16),
                  child: ErrorStateText(_relayError!),
                )
              else if (network.relays.isEmpty)
                const Padding(
                  padding: EdgeInsets.all(16),
                  child: Text('No relays connected yet.'),
                )
              else
                for (final relay in network.relays)
                  ListTile(
                    leading: Icon(
                      Icons.circle,
                      size: 12,
                      color: relay.connected
                          ? Colors.green
                          : theme.colorScheme.outline,
                    ),
                    title: Text(relay.url,
                        maxLines: 1, overflow: TextOverflow.ellipsis),
                    subtitle: Text(
                      relay.connected
                          ? '${relay.latencyMs} ms · last event ${relay.lastEventAt}s'
                          : 'disconnected',
                    ),
                    trailing: IconButton(
                      icon: const Icon(Icons.delete_outline),
                      tooltip: 'Remove relay',
                      onPressed: () => _removeRelay(relay.url),
                    ),
                  ),
            ],
          );
        },
      ),
    );
  }

  Widget _transportsTab() {
    final theme = Theme.of(context);
    return ListView(
      padding: const EdgeInsets.symmetric(vertical: 8),
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16),
          child:
              Text('Anonymity Transports', style: theme.textTheme.titleMedium),
        ),
        Consumer<NetworkService>(
          builder: (context, network, _) {
            return Column(
              children: [
                _transportRow(
                  context,
                  'I2P',
                  network.i2p,
                  'Local i2pd SOCKS proxy (127.0.0.1:7656)',
                ),
                _transportRow(
                  context,
                  'Freenet',
                  network.freenet,
                  'Local Freenet gateway (127.0.0.1:8888)',
                ),
              ],
            );
          },
        ),
        const Divider(height: 32),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16),
          child: Text('Mesh Bearers', style: theme.textTheme.titleMedium),
        ),
        if (!_transportsLoaded)
          Padding(
            padding: const EdgeInsets.all(16),
            child: TextButton.icon(
              onPressed: _loadTransports,
              icon: const Icon(Icons.refresh),
              label: const Text('Load transport status'),
            ),
          )
        else ...[
          if (_loadingBearers)
            const Padding(
              padding: EdgeInsets.all(16),
              child: Center(child: CircularProgressIndicator()),
            )
          else if (_bearers.isEmpty)
            const Padding(
              padding: EdgeInsets.all(16),
              child: Text('No multi-bearer status available.'),
            )
          else
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: Column(
                children: [
                  for (final entry in _bearers.entries)
                    ListTile(
                      dense: true,
                      title: Text(entry.key),
                      trailing: Text(entry.value.toString()),
                    ),
                ],
              ),
            ),
        ],
        const Divider(height: 32),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16),
          child: Text('Reticulum Mesh', style: theme.textTheme.titleMedium),
        ),
        Consumer<MeshService>(
          builder: (context, mesh, _) {
            return Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                ListTile(
                  dense: true,
                  title: const Text('Running'),
                  trailing: Text(mesh.running ? 'ON' : 'OFF'),
                ),
                ListTile(
                  dense: true,
                  title: const Text('Destination hash'),
                  trailing: Text(_shortHash(mesh.destinationHash)),
                ),
                ListTile(
                  dense: true,
                  title: const Text('Active routes'),
                  trailing: Text('${mesh.activeRoutes}'),
                ),
                ListTile(
                  dense: true,
                  title: const Text('Packets'),
                  trailing: Text('rx ${mesh.rxPackets} · tx ${mesh.txPackets}'),
                ),
                if (mesh.lastError != null)
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 16),
                    child: ErrorStateText(mesh.lastError!),
                  ),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: Wrap(
                    spacing: 8,
                    children: [
                      TextButton.icon(
                        onPressed:
                            mesh.running || _startingMesh ? null : _startMesh,
                        icon: _startingMesh
                            ? const SizedBox(
                                width: 18,
                                height: 18,
                                child:
                                    CircularProgressIndicator(strokeWidth: 2),
                              )
                            : const Icon(Icons.play_arrow),
                        label: const Text('Start Mesh'),
                      ),
                      TextButton.icon(
                        onPressed: _refreshMesh,
                        icon: const Icon(Icons.refresh),
                        label: const Text('Refresh'),
                      ),
                      TextButton.icon(
                        onPressed: _announceMesh,
                        icon: const Icon(Icons.campaign_outlined),
                        label: const Text('Announce'),
                      ),
                    ],
                  ),
                ),
              ],
            );
          },
        ),
      ],
    );
  }

  Widget _transportRow(
    BuildContext context,
    String name,
    bool? active,
    String detail,
  ) {
    final theme = Theme.of(context);
    return ListTile(
      leading: Icon(
        active == null
            ? Icons.help_outline
            : active
                ? Icons.check_circle
                : Icons.remove_circle_outline,
        color: active == null
            ? theme.colorScheme.outline
            : active
                ? Colors.green
                : theme.colorScheme.error,
      ),
      title: Text(name),
      subtitle: Text(detail),
      trailing: Text(
        active == null
            ? 'unknown'
            : active
                ? 'running'
                : 'stopped',
        style: theme.textTheme.bodySmall,
      ),
    );
  }

  Widget _diagnosticsTab() {
    final theme = Theme.of(context);
    return ListView(
      padding: const EdgeInsets.symmetric(vertical: 8),
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16),
          child: Text('System Diagnostics', style: theme.textTheme.titleMedium),
        ),
        if (_diagnostics.isEmpty)
          Padding(
            padding: const EdgeInsets.all(16),
            child: TextButton.icon(
              onPressed: _loadingDiagnostics ? null : _loadDiagnostics,
              icon: _loadingDiagnostics
                  ? const SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.memory),
              label: const Text('Load diagnostics'),
            ),
          )
        else
          Padding(
            padding: const EdgeInsets.all(16),
            child: SelectableText(
              _diagnostics,
              style: theme.textTheme.bodySmall?.copyWith(
                fontFamily: 'monospace',
              ),
            ),
          ),
      ],
    );
  }
}
