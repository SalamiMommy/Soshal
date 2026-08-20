import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../services/mesh_service.dart';
import '../services/events_service.dart';
import '../services/network_service.dart';
import '../services/p2p_service.dart';
import '../services/permissions_service.dart';
import '../services/session_service.dart';
import '../utils/format.dart';
import '../widgets/error_state_text.dart';
import 'widgets/wgpu_mesh_canvas.dart';

/// Network: relay connections, anonymity transports, and diagnostics.
class NetworkScreen extends StatefulWidget {
  /// Network screen.
  const NetworkScreen({super.key});

  @override
  State<NetworkScreen> createState() => _NetworkScreenState();
}

class _NetworkScreenState extends State<NetworkScreen> {
  P2pService? _p2p;
  final TextEditingController _relayUrl = TextEditingController();
  bool _loadingRelays = false;
  bool _loadingDiagnostics = false;
  bool _loadingBearers = false;
  String? _relayError;
  String _diagnostics = '';
  Map<String, dynamic> _bearers = {};
  bool _transportsLoaded = false;
  bool _startingMesh = false;
  bool _meshBusy = false;
  bool _p2pBusy = false;
  String? _meshAddress;
  String? _p2pInfo;
  String? _swarmInfo;
  final TextEditingController _destAddrField = TextEditingController();
  final TextEditingController _packetField =
      TextEditingController(text: '{"ping":true}');
  final TextEditingController _aspectAppField =
      TextEditingController(text: 'soshal');
  final TextEditingController _aspectField =
      TextEditingController(text: 'peers');
  final TextEditingController _manifestField = TextEditingController(
    text:
        '{"blob_hash":"0000000000000000000000000000000000000000000000000000000000000000","total_size":0,"chunks":[]}',
  );
  final TextEditingController _peersField = TextEditingController();
  final TextEditingController _outPathField =
      TextEditingController(text: '/tmp/soshal-swarm.bin');
  final TextEditingController _fountainManifestField = TextEditingController(
    text:
        '{"total_len":0,"symbol_size":1024,"num_source_symbols":0,"oti_data":[]}',
  );
  final TextEditingController _fountainPacketsField =
      TextEditingController(text: '[]');
  final TextEditingController _geohashLat =
      TextEditingController(text: '52.5200');
  final TextEditingController _geohashLng =
      TextEditingController(text: '13.4050');
  String? _geohashResult;

  @override
  void initState() {
    super.initState();
    _p2p = context.read<P2pService>();
    WidgetsBinding.instance.addPostFrameCallback((_) => _loadRelays());
  }

  @override
  void dispose() {
    _p2p?.stopPolling();
    _relayUrl.dispose();
    _destAddrField.dispose();
    _packetField.dispose();
    _aspectAppField.dispose();
    _aspectField.dispose();
    _manifestField.dispose();
    _peersField.dispose();
    _outPathField.dispose();
    _fountainManifestField.dispose();
    _fountainPacketsField.dispose();
    _geohashLat.dispose();
    _geohashLng.dispose();
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
          SnackBar(content: SelectableText(ok ? 'Relay added' : 'Could not add relay')),
        );
        if (ok) _relayUrl.clear();
      }
    } catch (e) {
      debugPrint('add relay: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Add relay: $e')));
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
            .showSnackBar(SnackBar(content: SelectableText('Start mesh: $e')));
      }
    }
    if (mounted) setState(() => _startingMesh = false);
  }

  Future<void> _setTransportMode(
      BuildContext context, NetworkService network, TransportMode mode) async {
    final ok = await network.setTransportMode(mode);
    if (ok) network.refreshResolvedTransport();
    if (!context.mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(
        content: Text(ok
            ? 'Transport mode: ${mode.label}'
            : 'Transport mode change failed')));
  }

  void _startMeshRelay(BuildContext context, NetworkService network) {
    final pubkey = context.read<SessionService>().activePubkey ?? '';
    if (pubkey.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Sign in first to start the relay')));
      return;
    }
    try {
      network.startMeshRelay(pubkey);
    } catch (e) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: SelectableText('Mesh relay: $e')));
    }
  }

  String _meshPeersSummary(Map<String, dynamic>? status) {
    final peers = status?['peers'];
    if (peers is! Map) return '0';
    final parts = <String>[];
    peers.forEach((kind, count) => parts.add('$kind $count'));
    return parts.isEmpty ? '0' : parts.join(' · ');
  }

  Future<void> _refreshMesh() async {
    try {
      await context.read<MeshService>().refreshStatus();
    } catch (e) {
      debugPrint('refresh mesh: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Refresh mesh: $e')));
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
            .showSnackBar(SnackBar(content: SelectableText('Announce: $e')));
      }
    }
  }

  Future<void> _meshAutoInterface() async {
    setState(() => _meshBusy = true);
    try {
      await context.read<MeshService>().startAutoInterface(
            enabled: true,
            port: 4242,
            intervalMs: 60000,
          );
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('AutoInterface started on :4242')));
      }
    } catch (e) {
      debugPrint('auto interface: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('AutoInterface: $e')));
      }
    }
    if (mounted) setState(() => _meshBusy = false);
  }

  Future<void> _meshTcpServer() async {
    setState(() => _meshBusy = true);
    try {
      await context
          .read<MeshService>()
          .startTcpServer(port: 4243, maxConnections: 8);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(const SnackBar(
            content: Text('Reticulum TCP server on :4243 (8 conns)')));
      }
    } catch (e) {
      debugPrint('tcp server: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('TCP server: $e')));
      }
    }
    if (mounted) setState(() => _meshBusy = false);
  }

  Future<void> _meshSendPacket() async {
    final dest = _destAddrField.text.trim();
    if (dest.isEmpty) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Enter a destination address')));
      }
      return;
    }
    try {
      final ok = await context.read<MeshService>().sendPacket(
            destAddr: dest,
            packetJson: _packetField.text,
          );
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText(ok ? 'Packet sent' : 'Packet send failed')));
      }
    } catch (e) {
      debugPrint('send packet: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Send packet: $e')));
      }
    }
  }

  Future<void> _meshRequestLink() async {
    final dest = _destAddrField.text.trim();
    if (dest.isEmpty) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Enter a destination address')));
      }
      return;
    }
    try {
      final link = await context.read<MeshService>().requestLink(dest);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content:
                Text(link == null ? 'Link request failed' : 'Link $link')));
      }
    } catch (e) {
      debugPrint('request link: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Request link: $e')));
      }
    }
  }

  Future<void> _meshAddressFromPubkey() async {
    try {
      final addr = await context.read<MeshService>().addressFromPubkey();
      if (mounted) setState(() => _meshAddress = addr);
    } catch (e) {
      debugPrint('address from pubkey: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Address: $e')));
      }
    }
  }

  Future<void> _meshAddressFromAspect() async {
    try {
      final addr = await context.read<MeshService>().addressFromAspect(
            _aspectAppField.text.trim(),
            _aspectField.text.trim(),
          );
      if (mounted) setState(() => _meshAddress = addr);
    } catch (e) {
      debugPrint('address from aspect: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Address: $e')));
      }
    }
  }

  Future<void> _meshAnnounceNetwork() async {
    final pubkey = context.read<SessionService>().activePubkey;
    if (pubkey == null) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Sign in first')));
      }
      return;
    }
    try {
      final ok =
          context.read<NetworkService>().reticulumAnnounce(pubkey: pubkey);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(ok ? 'Reticulum announce sent' : 'Announce failed')));
      }
    } catch (e) {
      debugPrint('reticulum announce: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Announce: $e')));
      }
    }
  }

  Future<void> _meshNetworkStatus() async {
    try {
      final json = context.read<NetworkService>().reticulumStatus();
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(json.length > 200
                ? 'Reticulum status: ${json.substring(0, 200)}…'
                : 'Reticulum status: $json')));
      }
    } catch (e) {
      debugPrint('reticulum status: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Status: $e')));
      }
    }
  }

  Future<void> _meshNetworkStop() async {
    try {
      final ok = context.read<NetworkService>().reticulumStop();
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(ok ? 'Reticulum transport stopped' : 'Stop failed')));
      }
    } catch (e) {
      debugPrint('reticulum stop: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Stop: $e')));
      }
    }
  }

  Future<void> _p2pStartServers() async {
    setState(() => _p2pBusy = true);
    try {
      final lanPort = await context.read<P2pService>().start();
      if (mounted) {
        final quicPort = context.read<P2pService>().quicPort;
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(
                'P2P servers up · LAN $lanPort · QUIC ${quicPort ?? '—'} · advertising')));
      }
    } catch (e) {
      debugPrint('p2p start: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Start P2P: $e')));
      }
    }
    if (mounted) setState(() => _p2pBusy = false);
  }

  Future<void> _p2pBrowse() async {
    final p2p = context.read<P2pService>();
    try {
      await p2p.startBrowsing();
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content:
                Text(p2p.browsing ? 'Browsing started' : 'Browse failed')));
      }
    } catch (e) {
      debugPrint('p2p browse: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Browse: $e')));
      }
    }
  }

  Future<void> _p2pDrain() async {
    final p2p = context.read<P2pService>();
    final found = await p2p.drainPeers();
    if (mounted) {
      setState(() {
        _p2pInfo = '${found.length} peer(s) on subnet';
        _peersField.text = p2p.peers.map((p) => '${p.ip}:${p.port}').join(',');
      });
    }
  }

  Future<void> _p2pPolling() async {
    context.read<P2pService>().startPolling(const Duration(seconds: 5));
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(const SnackBar(
          content:
              Text('Auto-refresh every 5s (Stop All stops the timer too)')));
    }
  }

  Future<void> _p2pStopAll() async {
    await context.read<P2pService>().stopAll();
    if (mounted) {
      setState(() {
        _p2pInfo = null;
        _swarmInfo = null;
      });
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: SelectableText('All P2P stopped')));
    }
  }

  Future<void> _p2pPower() async {
    final power = await context.read<P2pService>().currentPower();
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text(power == null
              ? 'No power snapshot'
              : 'Mode ${power.mode} · paused ${power.paused} · '
                  'maxParallel ${power.maxParallelUploads} · '
                  'budget ${power.uploadBudgetBytesPerSec} B/s')));
    }
  }

  Future<void> _p2pPushPower() async {
    final p2p = context.read<P2pService>();
    await p2p.refreshPowerFromOs();
    final power = p2p.power;
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text(power == null
              ? 'Power push failed'
              : 'Pushed · mode ${power.mode}')));
    }
  }

  Future<void> _p2pSwarmDownload() async {
    final p2p = context.read<P2pService>();
    final peers = _peersField.text
        .split(',')
        .map((s) => s.trim())
        .where((s) => s.isNotEmpty)
        .toList();
    if (peers.isEmpty) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('No peers — drain the subnet first')));
      }
      return;
    }
    setState(() => _p2pBusy = true);
    try {
      final quicPorts = <int?>[];
      for (final entry in peers) {
        int? quic;
        for (final peer in p2p.peers) {
          if ('${peer.ip}:${peer.port}' == entry) {
            quic = peer.quicPort;
            break;
          }
        }
        quicPorts.add(quic);
      }
      final id = await p2p.swarmDownload(
        manifestJson: _manifestField.text,
        peers: peers,
        quicPorts: quicPorts,
        outPath: _outPathField.text,
      );
      if (mounted) {
        setState(() => _swarmInfo = 'Download $id running');
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Swarm download $id started')));
      }
    } catch (e) {
      debugPrint('swarm download: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Swarm download: $e')));
      }
    }
    if (mounted) setState(() => _p2pBusy = false);
  }

  Future<void> _p2pSwarmPoll() async {
    final p2p = context.read<P2pService>();
    final id = p2p.downloads.keys.isNotEmpty ? p2p.downloads.keys.first : null;
    if (id == null) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('No active downloads')));
      }
      return;
    }
    final status = await p2p.swarmStatus(id);
    if (mounted) {
      setState(() => _swarmInfo = status == null
          ? 'Poll $id failed'
          : '$id · ${status.state} · ${status.verifiedChunks} chunks · '
              '${status.bytesDownloaded} B · ${status.failures} failures');
    }
  }

  Future<void> _p2pSwarmCancel() async {
    final p2p = context.read<P2pService>();
    final id = p2p.downloads.keys.isNotEmpty ? p2p.downloads.keys.first : null;
    if (id == null) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('No active downloads')));
      }
      return;
    }
    await p2p.swarmCancel(id);
    if (mounted) {
      setState(() => _swarmInfo = null);
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: SelectableText('Cancelled $id')));
    }
  }

  Future<void> _fountainEncode() async {
    final data =
        utf8.encode('Soshal mesh fountain round-trip payload 0123456789');
    final manifest = await context.read<P2pService>().encodeFountainPayload(
          data: data,
          redundancyRatio: 1.2,
        );
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text(manifest['success'] == false
              ? 'Encode failed: ${manifest['error_msg']}'
              : 'Encoded · ${manifest['total_len']} B · '
                  '${manifest['num_source_symbols']} symbols · '
                  'symbol ${manifest['symbol_size']} B')));
    }
  }

  Future<void> _fountainDecode() async {
    final bytes = await context.read<P2pService>().decodeFountainPayload(
          manifestJson: _fountainManifestField.text,
          packetsB64Json: _fountainPacketsField.text,
        );
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text(bytes == null
              ? 'Decode failed (supply a real manifest + base64 packets)'
              : 'Decoded ${bytes.length} bytes')));
    }
  }

  Future<void> _useMyLocation() async {
    try {
      final location = await PermissionsService.currentPosition();
      if (!location.ok) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(SnackBar(
              content: Text('Location unavailable: ${location.error}')));
        }
        return;
      }
      if (!mounted) return;
      _geohashLat.text = location.latitude!.toStringAsFixed(6);
      _geohashLng.text = location.longitude!.toStringAsFixed(6);
      await _encodeGeohash();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Location failed: $e')));
      }
    }
  }

  Future<void> _encodeGeohash() async {
    final lat = double.tryParse(_geohashLat.text.trim());
    final lng = double.tryParse(_geohashLng.text.trim());
    if (lat == null || lng == null) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Enter valid lat/lng numbers')));
      }
      return;
    }
    try {
      final geohash =
          context.read<EventsService>().encodeGeohash(lat: lat, lon: lng);
      if (mounted) setState(() => _geohashResult = geohash);
    } catch (e) {
      debugPrint('encode geohash: $e');
      if (mounted) {
        setState(() => _geohashResult = 'Encode failed: $e');
      }
    }
  }

  List<WgpuMeshNodeItem> _meshNodes(MeshService mesh, P2pService p2p) {
    final nodes = <WgpuMeshNodeItem>[];
    if (mesh.destinationHash != null && mesh.destinationHash!.isNotEmpty) {
      nodes.add(
          WgpuMeshNodeItem(id: 'self', x: 0.0, y: 0.0, z: 0.0, latencyMs: 0));
    }
    var i = 0;
    for (final peer in p2p.peers.take(16)) {
      final h = peer.pubkey.hashCode + i * 97;
      nodes.add(WgpuMeshNodeItem(
        id: 'p$i',
        x: ((h % 100) - 50) / 100.0,
        y: ((h ~/ 7 % 100) - 50) / 100.0,
        z: ((h ~/ 13 % 40) - 20) / 100.0,
        latencyMs: 15,
      ));
      i++;
    }
    return nodes;
  }

  Widget _meshTopologyCanvas() {
    return LayoutBuilder(
      builder: (context, constraints) {
        final w = constraints.maxWidth > 0 ? constraints.maxWidth : 320.0;
        try {
          return Consumer2<MeshService, P2pService>(
            builder: (context, mesh, p2p, _) {
              return WgpuMeshCanvasWidget(
                width: w,
                height: 240,
                nodes: _meshNodes(mesh, p2p),
              );
            },
          );
        } catch (e) {
          return Container(
            height: 240,
            alignment: Alignment.center,
            child: Text('WGPU mesh canvas unavailable: $e'),
          );
        }
      },
    );
  }

  String _shortHash(String? hash) {
    if (hash == null || hash.isEmpty) return '—';
    return prefixEllipsis(hash, 16);
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
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: Wrap(
                    spacing: 8,
                    children: [
                      for (final mode in TransportMode.values)
                        ChoiceChip(
                          label: Text(mode.label),
                          selected: network.transportMode == mode,
                          onSelected: (_) =>
                              _setTransportMode(context, network, mode),
                        ),
                    ],
                  ),
                ),
                if (network.resolved != null)
                  ListTile(
                    dense: true,
                    title: Text(
                        'In use: ${network.resolved!.resolved.toUpperCase()}'),
                    subtitle: Text(network.resolved!.satisfied
                        ? 'Mode ${network.transportMode.label}'
                        : 'Preferred transport down - fell back to Nostr'),
                    trailing: Icon(
                      network.resolved!.satisfied
                          ? Icons.check_circle
                          : Icons.warning_amber,
                      color: network.resolved!.satisfied
                          ? Colors.green
                          : Colors.orange,
                    ),
                  ),
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
                ListTile(
                  dense: true,
                  leading: const Icon(Icons.hub_outlined),
                  title: const Text('Mesh relay (device-as-relay)'),
                  subtitle: Text(network.meshRelayRunning
                      ? 'running - peers ${_meshPeersSummary(network.meshStatus)}'
                      : 'stopped'),
                  trailing: network.meshRelayRunning
                      ? TextButton(
                          onPressed: network.stopMeshRelay,
                          child: const Text('Stop'),
                        )
                      : TextButton(
                          onPressed: () => _startMeshRelay(context, network),
                          child: const Text('Start'),
                        ),
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
                      TextButton.icon(
                        onPressed: _meshNetworkStop,
                        icon: const Icon(Icons.stop_circle_outlined),
                        label: const Text('Stop'),
                      ),
                      TextButton.icon(
                        onPressed: _meshNetworkStatus,
                        icon: const Icon(Icons.query_stats),
                        label: const Text('Status'),
                      ),
                      TextButton.icon(
                        onPressed: _meshAnnounceNetwork,
                        icon: const Icon(Icons.rss_feed),
                        label: const Text('Announce (pubkey)'),
                      ),
                      TextButton.icon(
                        onPressed: _meshBusy ? null : _meshAutoInterface,
                        icon: const Icon(Icons.wifi_tethering),
                        label: const Text('AutoInterface'),
                      ),
                      TextButton.icon(
                        onPressed: _meshBusy ? null : _meshTcpServer,
                        icon: const Icon(Icons.dns_outlined),
                        label: const Text('TCP Server'),
                      ),
                    ],
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: TextField(
                    controller: _destAddrField,
                    decoration: const InputDecoration(
                      hintText: 'Destination address (hex)',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: Row(
                    children: [
                      Expanded(
                        child: TextField(
                          controller: _packetField,
                          decoration: const InputDecoration(
                            labelText: 'Packet JSON',
                            border: OutlineInputBorder(),
                            isDense: true,
                          ),
                        ),
                      ),
                      const SizedBox(width: 8),
                      IconButton(
                        icon: const Icon(Icons.send),
                        tooltip: 'Send packet',
                        onPressed: _meshSendPacket,
                      ),
                      IconButton(
                        icon: const Icon(Icons.link),
                        tooltip: 'Request link',
                        onPressed: _meshRequestLink,
                      ),
                    ],
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: Row(
                    children: [
                      Expanded(
                        child: TextField(
                          controller: _aspectAppField,
                          decoration: const InputDecoration(
                            labelText: 'App name',
                            border: OutlineInputBorder(),
                            isDense: true,
                          ),
                        ),
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: TextField(
                          controller: _aspectField,
                          decoration: const InputDecoration(
                            labelText: 'Aspect',
                            border: OutlineInputBorder(),
                            isDense: true,
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: Wrap(
                    spacing: 8,
                    children: [
                      TextButton.icon(
                        onPressed: _meshAddressFromPubkey,
                        icon: const Icon(Icons.person_pin_circle_outlined),
                        label: const Text('Address from pubkey'),
                      ),
                      TextButton.icon(
                        onPressed: _meshAddressFromAspect,
                        icon: const Icon(Icons.tag),
                        label: const Text('Address from aspect'),
                      ),
                    ],
                  ),
                ),
                if (_meshAddress != null)
                  ListTile(
                    dense: true,
                    title: const Text('Reticulum address'),
                    subtitle: SelectableText(_meshAddress!),
                  ),
              ],
            );
          },
        ),
        const Divider(height: 32),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16),
          child: Text('Mesh Topology', style: theme.textTheme.titleMedium),
        ),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
          child: _meshTopologyCanvas(),
        ),
        const Divider(height: 32),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16),
          child: Text('Local P2P (LAN)', style: theme.textTheme.titleMedium),
        ),
        Consumer<P2pService>(
          builder: (context, p2p, _) {
            return Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                ListTile(
                  dense: true,
                  title: const Text('LAN chunk server'),
                  trailing: Text(
                      p2p.lanPort != null ? 'port ${p2p.lanPort}' : 'stopped'),
                ),
                ListTile(
                  dense: true,
                  title: const Text('QUIC stream server'),
                  trailing: Text(p2p.quicPort != null
                      ? 'port ${p2p.quicPort}'
                      : 'stopped'),
                ),
                ListTile(
                  dense: true,
                  title: const Text('Advertising'),
                  trailing: Text(p2p.advertising ? 'ON' : 'OFF'),
                ),
                ListTile(
                  dense: true,
                  title: const Text('Browsing'),
                  trailing: Text(p2p.browsing ? 'ON' : 'OFF'),
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: Wrap(
                    spacing: 8,
                    children: [
                      TextButton.icon(
                        onPressed: _p2pBusy ? null : _p2pStartServers,
                        icon: const Icon(Icons.play_arrow),
                        label: const Text('Start Servers'),
                      ),
                      TextButton.icon(
                        onPressed: _p2pBrowse,
                        icon: const Icon(Icons.search),
                        label: const Text('Browse'),
                      ),
                      TextButton.icon(
                        onPressed: _p2pDrain,
                        icon: const Icon(Icons.people_outline),
                        label: const Text('Drain Peers'),
                      ),
                      TextButton.icon(
                        onPressed: _p2pPolling,
                        icon: const Icon(Icons.autorenew),
                        label: const Text('Auto-refresh'),
                      ),
                      TextButton.icon(
                        onPressed: _p2pPushPower,
                        icon: const Icon(Icons.battery_charging_full),
                        label: const Text('Push Power'),
                      ),
                      TextButton.icon(
                        onPressed: _p2pPower,
                        icon: const Icon(Icons.power),
                        label: const Text('Power Mode'),
                      ),
                      TextButton.icon(
                        onPressed: _p2pStopAll,
                        icon: const Icon(Icons.stop_circle_outlined),
                        label: const Text('Stop All'),
                      ),
                    ],
                  ),
                ),
                if (_p2pInfo != null)
                  ListTile(
                    dense: true,
                    title: Text(_p2pInfo!),
                    trailing: IconButton(
                      icon: const Icon(Icons.close),
                      tooltip: 'Clear',
                      onPressed: () => setState(() => _p2pInfo = null),
                    ),
                  ),
                if (p2p.peers.isEmpty)
                  const Padding(
                    padding: EdgeInsets.symmetric(horizontal: 16),
                    child: Text('No LAN peers discovered yet.'),
                  )
                else
                  for (final peer in p2p.peers)
                    ListTile(
                      dense: true,
                      leading: const Icon(Icons.devices_other),
                      title: Text('${peer.ip}:${peer.port}'),
                      subtitle: Text(
                        '${prefixEllipsis(peer.pubkey, 12)}'
                        ' · QUIC ${peer.quicPort ?? '—'}',
                      ),
                    ),
                const Divider(height: 24),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: Text('Swarm Downloads',
                      style: theme.textTheme.titleSmall),
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: TextField(
                    controller: _manifestField,
                    maxLines: 2,
                    decoration: const InputDecoration(
                      labelText: 'Manifest JSON',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: TextField(
                    controller: _peersField,
                    decoration: const InputDecoration(
                      labelText: 'Peers (ip:port, comma-separated)',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: TextField(
                    controller: _outPathField,
                    decoration: const InputDecoration(
                      labelText: 'Output path',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: Wrap(
                    spacing: 8,
                    children: [
                      TextButton.icon(
                        onPressed: _p2pBusy ? null : _p2pSwarmDownload,
                        icon: const Icon(Icons.download),
                        label: const Text('Download'),
                      ),
                      TextButton.icon(
                        onPressed: _p2pSwarmPoll,
                        icon: const Icon(Icons.poll),
                        label: const Text('Poll'),
                      ),
                      TextButton.icon(
                        onPressed: _p2pSwarmCancel,
                        icon: const Icon(Icons.cancel_outlined),
                        label: const Text('Cancel'),
                      ),
                    ],
                  ),
                ),
                if (_swarmInfo != null)
                  ListTile(
                    dense: true,
                    title: Text(_swarmInfo!),
                    subtitle:
                        Text('${p2p.downloads.length} active download(s)'),
                  ),
                const Divider(height: 24),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child:
                      Text('Fountain Codes', style: theme.textTheme.titleSmall),
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: TextField(
                    controller: _fountainManifestField,
                    maxLines: 2,
                    decoration: const InputDecoration(
                      labelText: 'Fountain manifest JSON',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: TextField(
                    controller: _fountainPacketsField,
                    decoration: const InputDecoration(
                      labelText: 'Packets (base64 JSON list)',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: Wrap(
                    spacing: 8,
                    children: [
                      TextButton.icon(
                        onPressed: _fountainEncode,
                        icon: const Icon(Icons.bolt),
                        label: const Text('Encode test payload'),
                      ),
                      TextButton.icon(
                        onPressed: _fountainDecode,
                        icon: const Icon(Icons.data_object),
                        label: const Text('Decode'),
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
        const Divider(height: 32),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16),
          child: Text('Network Tools', style: theme.textTheme.titleMedium),
        ),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _geohashLat,
                  keyboardType: const TextInputType.numberWithOptions(
                      decimal: true, signed: true),
                  decoration: const InputDecoration(
                    labelText: 'Latitude',
                    border: OutlineInputBorder(),
                    isDense: true,
                  ),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: TextField(
                  controller: _geohashLng,
                  keyboardType: const TextInputType.numberWithOptions(
                      decimal: true, signed: true),
                  decoration: const InputDecoration(
                    labelText: 'Longitude',
                    border: OutlineInputBorder(),
                    isDense: true,
                  ),
                ),
              ),
              const SizedBox(width: 8),
              IconButton(
                icon: const Icon(Icons.gps_fixed),
                tooltip: 'Use my location',
                onPressed: _useMyLocation,
              ),
              const SizedBox(width: 8),
              IconButton(
                icon: const Icon(Icons.my_location),
                tooltip: 'Encode geohash',
                onPressed: _encodeGeohash,
              ),
            ],
          ),
        ),
        if (_geohashResult != null)
          ListTile(
            dense: true,
            title: Text('Geohash: ${_geohashResult!}'),
            trailing: IconButton(
              icon: const Icon(Icons.close),
              tooltip: 'Clear',
              onPressed: () => setState(() => _geohashResult = null),
            ),
          ),
      ],
    );
  }
}
