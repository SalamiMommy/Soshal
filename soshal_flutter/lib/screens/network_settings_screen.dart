import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/daemon_service.dart';
import '../services/ebpf_service.dart';
import '../services/network_service.dart';
import '../services/permissions_service.dart';
import '../services/session_service.dart';
import '../services/shell_service.dart';
import '../utils/format.dart';
import '../widgets/error_state_text.dart';

/// Network settings — relay management (add/remove), connection status, and
/// transport toggles. Port of the legacy network settings section.
class NetworkSettingsScreen extends StatefulWidget {
  const NetworkSettingsScreen({super.key});

  @override
  State<NetworkSettingsScreen> createState() => _NetworkSettingsScreenState();
}

class _NetworkSettingsScreenState extends State<NetworkSettingsScreen> {
  final TextEditingController _relayField = TextEditingController();
  final TextEditingController _ebpfIp = TextEditingController();
  final TextEditingController _http3Url =
      TextEditingController(text: 'https://example.com');
  final TextEditingController _localKvField =
      TextEditingController(text: '{"self":"test"}');
  final TextEditingController _remoteHashField = TextEditingController(
    text: '0000000000000000000000000000000000000000000000000000000000000000',
  );
  final TextEditingController _freenetUrl =
      TextEditingController(text: 'ws://127.0.0.1:7509');
  final TextEditingController _freenetAuthToken = TextEditingController();
  final TextEditingController _freenetKey = TextEditingController();
  final TextEditingController _freenetStateField =
      TextEditingController(text: '{}');
  final TextEditingController _freenetSummaryField =
      TextEditingController(text: '{}');
  final TextEditingController _samHost =
      TextEditingController(text: '127.0.0.1');
  final TextEditingController _samPort = TextEditingController(text: '7656');
  final TextEditingController _sessionId =
      TextEditingController(text: 'soshal-sam');
  final TextEditingController _samDestField = TextEditingController();
  final TextEditingController _relayFilterField =
      TextEditingController(text: '{"kinds":[1],"limit":5}');
  final TextEditingController _subscriptionIdField = TextEditingController();
  final TextEditingController _eventJsonField =
      TextEditingController(text: '{}');
  final TextEditingController _bleBeaconField =
      TextEditingController(text: '{}');
  final TextEditingController _bleRootField = TextEditingController(
    text: '0000000000000000000000000000000000000000000000000000000000000000',
  );
  final TextEditingController _zkProofField = TextEditingController(text: '{}');
  final TextEditingController _zkRootField = TextEditingController(
    text: '0000000000000000000000000000000000000000000000000000000000000000',
  );
  final TextEditingController _zkNullifiersField =
      TextEditingController(text: '[]');
  bool _loading = true;
  bool _busy = false;
  String? _error;
  String? _i2pDest;
  String? _http3Info;
  String? _reconcileInfo;
  String? _freenetInfo;
  String? _samInfo;
  String? _rawRelayInfo;
  String? _bleInfo;
  String? _zkInfo;
  bool _daemonBusy = false;
  bool? _daemonsAvailable;
  Map<String, dynamic> _daemonStatus = {};
  bool? _i2pdRunning;
  bool? _rnsdRunning;
  String? _rnsdError;
  bool? _serviceRunning;
  String? _daemonInfo;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _refresh());
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    final service = context.read<NetworkService>();
    try {
      await service.loadTransportMode();
      await service.refresh();
      await service.fetchRelayStatus();
      _error = null;
    } catch (e) {
      _error = '$e';
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _setTransportMode(TransportMode mode) async {
    final service = context.read<NetworkService>();
    final ok = await service.setTransportMode(mode);
    if (ok) service.refreshResolvedTransport();
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(
        content: Text(ok
            ? 'Transport mode: ${mode.label}'
            : 'Transport mode change failed')));
  }

  void _startMeshRelay(NetworkService service) {
    final pubkey = context.read<SessionService>().activePubkey ?? '';
    if (pubkey.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: SelectableText('Sign in first to start the relay')));
      return;
    }
    try {
      service.startMeshRelay(pubkey);
    } catch (e) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: SelectableText('Mesh relay: $e')));
    }
  }

  String _meshPeersSummary(Map<String, dynamic> status) {
    final peers = status['peers'];
    if (peers is! Map) return '0';
    final parts = <String>[];
    peers.forEach((kind, count) => parts.add('$kind $count'));
    return parts.isEmpty ? '0' : parts.join(' · ');
  }

  Future<void> _startI2p() async {
    setState(() => _busy = true);
    try {
      final dest = await context.read<NetworkService>().startI2pSession();
      if (mounted) setState(() => _i2pDest = dest);
    } catch (e) {
      debugPrint('i2p start: $e');
      if (mounted) {
        final hint = e.toString().contains('Connection refused')
            ? ' — run i2pd (SAM on 7656) first'
            : '';
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('I2P start: $e$hint')));
      }
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _stopI2p() async {
    try {
      final ok = await context.read<NetworkService>().stopI2pSession();
      if (mounted) {
        setState(() => _i2pDest = ok ? null : _i2pDest);
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(ok ? 'I2P session stopped' : 'Stop failed')));
      }
    } catch (e) {
      debugPrint('i2p stop: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('I2P stop: $e')));
      }
    }
  }

  Future<void> _i2pStatus() async {
    try {
      final status = await context.read<NetworkService>().i2pSessionStatus();
      if (mounted) {
        setState(() => _i2pDest = status['destination']?.toString());
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text('I2P session running=${status['running']}')));
      }
    } catch (e) {
      debugPrint('i2p status: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('I2P status: $e')));
      }
    }
  }

  Future<void> _reinitRelays() async {
    try {
      final shell = context.read<ShellService>();
      await context.read<NetworkService>().reinitRelays();
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Relays reconnected')));
      }
      // Reconnect may take seconds; refresh the banner immediately instead of
      // waiting up to 15 s for the periodic poll.
      shell.refreshRelayStatus();
      await _refresh();
    } catch (e) {
      debugPrint('reinit relays: $e');
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Reinit relays: $e')));
      }
    }
  }

  Future<void> _http3Fetch() async {
    setState(() => _busy = true);
    try {
      final resp = await context
          .read<NetworkService>()
          .fetchHttp3(_http3Url.text.trim());
      if (mounted) {
        setState(
            () => _http3Info = 'HTTP ${resp.status} · ${resp.body.length} B');
      }
    } catch (e) {
      debugPrint('http3 fetch: $e');
      if (mounted) {
        setState(() => _http3Info = 'HTTP/3 fetch failed: $e');
      }
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _reconcile() async {
    Map<String, String> kv;
    try {
      kv = Map<String, String>.from(
          jsonDecode(_localKvField.text) as Map<String, dynamic>);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: SelectableText('Local KV must be a JSON object')));
      }
      return;
    }
    setState(() => _busy = true);
    final res = await context.read<NetworkService>().reconcileProllyTree(
          localKv: kv,
          remoteRootHash: _remoteHashField.text.trim(),
        );
    if (mounted) {
      setState(() => _reconcileInfo = res['success'] == true
          ? 'Reconciled: $res'
          : 'Reconcile failed: ${res['error_msg']}');
      setState(() => _busy = false);
    }
  }

  Future<void> _freenetConnect() async {
    setState(() => _busy = true);
    try {
      final ok = await context.read<NetworkService>().freenetConnect(
            url: _freenetUrl.text.trim(),
            authToken: _freenetAuthToken.text.trim(),
          );
      if (mounted) {
        setState(() => _freenetInfo = ok
            ? 'Freenet: connected'
            : 'Freenet: connect failed (needs local gateway on 7509)');
      }
    } catch (e) {
      debugPrint('freenet connect: $e');
      if (mounted) setState(() => _freenetInfo = 'Freenet connect: $e');
    } finally {
      _freenetAuthToken.clear();
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _freenetGetContract() async {
    setState(() => _busy = true);
    try {
      final json = await context.read<NetworkService>().freenetGetContract(
            url: _freenetUrl.text.trim(),
            authToken: _freenetAuthToken.text.trim(),
            key: _freenetKey.text.trim(),
            subscribe: false,
          );
      if (mounted) {
        setState(() => _freenetInfo = json.isEmpty
            ? 'Contract: empty'
            : 'Contract: ${json.length > 300 ? '${json.substring(0, 300)}…' : json}');
      }
    } catch (e) {
      debugPrint('freenet get contract: $e');
      if (mounted) setState(() => _freenetInfo = 'Contract get: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _freenetPutContract() async {
    setState(() => _busy = true);
    try {
      final json = await context.read<NetworkService>().freenetPutContract(
            url: _freenetUrl.text.trim(),
            authToken: _freenetAuthToken.text.trim(),
            stateJson: _freenetStateField.text,
            subscribe: false,
          );
      if (mounted) {
        setState(() => _freenetInfo = json.isEmpty
            ? 'Contract: put ok'
            : 'Contract put: ${json.length > 300 ? '${json.substring(0, 300)}…' : json}');
      }
    } catch (e) {
      debugPrint('freenet put contract: $e');
      if (mounted) setState(() => _freenetInfo = 'Contract put: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _freenetSubscribe() async {
    setState(() => _busy = true);
    try {
      final ok = await context.read<NetworkService>().freenetSubscribe(
            url: _freenetUrl.text.trim(),
            authToken: _freenetAuthToken.text.trim(),
            key: _freenetKey.text.trim(),
            summaryJson: _freenetSummaryField.text,
          );
      if (mounted) {
        setState(() => _freenetInfo = ok
            ? 'Freenet: subscribed to ${_freenetKey.text.trim()}'
            : 'Freenet: subscribe failed');
      }
    } catch (e) {
      debugPrint('freenet subscribe: $e');
      if (mounted) setState(() => _freenetInfo = 'Freenet subscribe: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _samConnect() async {
    final service = context.read<NetworkService>();
    try {
      final ok = service.i2pConnect(
        samHost: _samHost.text.trim(),
        samPort: int.tryParse(_samPort.text.trim()) ?? 7656,
      );
      if (mounted) {
        setState(
            () => _samInfo = ok ? 'SAM: connected' : 'SAM: connect failed');
      }
    } catch (e) {
      debugPrint('sam connect: $e');
      if (mounted) {
        setState(() => _samInfo = e.toString().contains('Connection refused')
            ? 'SAM: no listener on port — run i2pd first'
            : 'SAM: $e');
      }
    }
  }

  Future<void> _samCreateSession() async {
    try {
      final dest = context.read<NetworkService>().i2pCreateSession(
            samHost: _samHost.text.trim(),
            samPort: int.tryParse(_samPort.text.trim()) ?? 7656,
            sessionId: _sessionId.text.trim(),
          );
      if (mounted) {
        setState(() => _samInfo = dest.isEmpty
            ? 'SAM: session created'
            : 'SAM: session created · dest $dest');
      }
    } catch (e) {
      debugPrint('sam create session: $e');
      if (mounted) setState(() => _samInfo = 'SAM create session: $e');
    }
  }

  Future<void> _samGenerateDestination() async {
    try {
      final dest = context.read<NetworkService>().i2pGenerateDestination(
            samHost: _samHost.text.trim(),
            samPort: int.tryParse(_samPort.text.trim()) ?? 7656,
          );
      if (mounted) {
        setState(() {
          _samInfo = 'Generated destination (${dest.length} chars)';
          _samDestField.text = dest;
        });
      }
    } catch (e) {
      debugPrint('sam generate destination: $e');
      if (mounted) setState(() => _samInfo = 'Generate destination: $e');
    }
  }

  Future<void> _samConnectToDestination() async {
    final dest = _samDestField.text.trim();
    if (dest.isEmpty) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: SelectableText('Generate or paste a destination')));
      }
      return;
    }
    try {
      final ok = context.read<NetworkService>().i2pConnectToDestination(
            samHost: _samHost.text.trim(),
            samPort: int.tryParse(_samPort.text.trim()) ?? 7656,
            sessionId: _sessionId.text.trim(),
            destination: dest,
          );
      if (mounted) {
        setState(
            () => _samInfo = ok ? 'SAM: tunnel open' : 'SAM: tunnel failed');
      }
    } catch (e) {
      debugPrint('sam connect to destination: $e');
      if (mounted) setState(() => _samInfo = 'SAM tunnel: $e');
    }
  }

  Future<void> _relaySubscribe() async {
    setState(() => _busy = true);
    try {
      final id = await context
          .read<NetworkService>()
          .relaySubscribe(filterJson: _relayFilterField.text);
      if (mounted) {
        setState(() {
          _rawRelayInfo = 'Subscribed: $id';
          _subscriptionIdField.text = id;
        });
      }
    } catch (e) {
      debugPrint('relay subscribe: $e');
      if (mounted) setState(() => _rawRelayInfo = 'Subscribe: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _relayUnsubscribe() async {
    final id = _subscriptionIdField.text.trim();
    if (id.isEmpty) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Enter a subscription id')));
      }
      return;
    }
    setState(() => _busy = true);
    try {
      final ok = await context
          .read<NetworkService>()
          .relayUnsubscribe(subscriptionId: id);
      if (mounted) {
        setState(() =>
            _rawRelayInfo = ok ? 'Unsubscribed $id' : 'Unsubscribe failed');
      }
    } catch (e) {
      debugPrint('relay unsubscribe: $e');
      if (mounted) setState(() => _rawRelayInfo = 'Unsubscribe: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _relayPublishEvent() async {
    setState(() => _busy = true);
    try {
      final id = await context
          .read<NetworkService>()
          .publishEvent(eventJson: _eventJsonField.text);
      if (mounted) {
        setState(() => _rawRelayInfo = 'Published event $id');
      }
    } catch (e) {
      debugPrint('publish event: $e');
      if (mounted) setState(() => _rawRelayInfo = 'Publish: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _relayQueryEvents() async {
    setState(() => _busy = true);
    try {
      final json = await context
          .read<NetworkService>()
          .queryEvents(filterJson: _relayFilterField.text);
      if (mounted) {
        setState(() => _rawRelayInfo = json.length > 300
            ? '${json.substring(0, 300)}… (${json.length} B)'
            : json);
      }
    } catch (e) {
      debugPrint('query events: $e');
      if (mounted) setState(() => _rawRelayInfo = 'Query: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _processBleBeacon() async {
    final ownPubkey = context.read<SessionService>().activePubkey;
    if (ownPubkey == null) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: SelectableText('Sign in first (need own pubkey)')));
      }
      return;
    }
    setState(() => _busy = true);
    try {
      final result = await context.read<NetworkService>().processBleBeacon(
            beacon: _bleBeaconField.text,
            localRootHex: _bleRootField.text.trim(),
            ownPubkey: ownPubkey,
          );
      if (mounted) {
        setState(() => _bleInfo = result ?? 'Beacon processed (no reply)');
      }
    } catch (e) {
      debugPrint('ble beacon: $e');
      if (mounted) setState(() => _bleInfo = 'BLE beacon: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _verifyZkWot() async {
    setState(() => _busy = true);
    try {
      final ok = await context.read<NetworkService>().verifyZkWotProof(
            proofJson: _zkProofField.text,
            expectedWotRoot: _zkRootField.text.trim(),
            blacklistedNullifiersJson: _zkNullifiersField.text,
          );
      if (mounted) {
        setState(() =>
            _zkInfo = ok ? 'ZK WoT proof: valid' : 'ZK WoT proof: invalid');
      }
    } catch (e) {
      debugPrint('zk wot verify: $e');
      if (mounted) setState(() => _zkInfo = 'ZK WoT verify: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _notifyInterfaceChange() async {
    final ok = await context.read<NetworkService>().notifyInterfaceChange();
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text(ok ? 'Interface change notified' : 'Notify failed')));
    }
  }

  /// Extracts the `error` field from `RnsdRunner.status()`'s JSON fragment
  /// (`"running":true,"error":"..."`) or falls back to the raw text.
  String? _parseRnsdError(String fragment) {
    final trimmed = fragment.trim();
    if (trimmed.isEmpty) return null;
    try {
      // Wrap in braces to make the fragment valid JSON.
      final decoded = jsonDecode('{$trimmed}') as Map<String, dynamic>;
      final error = decoded['error'] as String?;
      if (error != null && error.trim().isNotEmpty) return error;
      return null;
    } catch (_) {}
    if (trimmed == '"running":false' || trimmed == '"running":true') {
      return null;
    }
    return trimmed;
  }

  Future<void> _showDaemonLogs() async {
    final logs = await DaemonService.readDaemonLogs();
    if (!mounted) return;
    final labels = {
      DaemonService.i2pd: 'i2pd',
      DaemonService.freenet: 'freenet',
      DaemonService.reticulum: 'rnsd',
    };
    if (!mounted) return;
    await showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      constraints:
          BoxConstraints(maxHeight: MediaQuery.of(context).size.height * 0.8),
      builder: (ctx) {
        return StatefulBuilder(
          builder: (ctx, setSheetState) {
            return Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text('Daemon logs',
                      style: Theme.of(ctx).textTheme.titleMedium),
                  const SizedBox(height: 4),
                  if (_rnsdError != null)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 8),
                      child: ErrorStateText('rnsd: $_rnsdError'),
                    ),
                  Expanded(
                    child: ListView(
                      children: [
                        for (final entry in logs.entries)
                          Padding(
                            padding: const EdgeInsets.only(bottom: 16),
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(
                                  '${labels[entry.key] ?? entry.key} — '
                                  '${entry.value.isEmpty ? 'no log yet' : 'last ${entry.value.split('\n').length} lines'}',
                                  style: Theme.of(ctx).textTheme.titleSmall,
                                ),
                                if (entry.value.isNotEmpty)
                                  SelectableText(
                                    entry.value,
                                    style: const TextStyle(
                                        fontFamily: 'monospace', fontSize: 11),
                                  ),
                              ],
                            ),
                          ),
                      ],
                    ),
                  ),
                  Row(
                    mainAxisAlignment: MainAxisAlignment.end,
                    children: [
                      TextButton(
                        onPressed: () async {
                          final fresh = await DaemonService.readDaemonLogs();
                          if (ctx.mounted) {
                            setSheetState(() => logs..addAll(fresh));
                          }
                        },
                        child: const Text('Reload'),
                      ),
                      TextButton(
                        onPressed: () => Navigator.of(ctx).pop(),
                        child: const Text('Close'),
                      ),
                    ],
                  ),
                ],
              ),
            );
          },
        );
      },
    );
  }

  Future<void> _daemonsRefresh() async {
    setState(() => _daemonBusy = true);
    try {
      final available = await DaemonService.areDaemonsAvailable();
      final status = await DaemonService.getDaemonStatus();
      final i2pd = await DaemonService.isI2pdRunning();
      final rnsd = await DaemonService.isRnsdRunning();
      final service = await DaemonService.isServiceRunning();
      if (mounted) {
        setState(() {
          _daemonsAvailable = available;
          _daemonStatus = status;
          _i2pdRunning = i2pd;
          _rnsdRunning = rnsd;
          final rnsdStatus = status['reticulum_status'] as String?;
          _rnsdError = rnsdStatus == null ? null : _parseRnsdError(rnsdStatus);
          _serviceRunning = service;
          _daemonInfo = null;
        });
      }
    } catch (e) {
      debugPrint('daemon status: $e');
      if (mounted) setState(() => _daemonInfo = 'Daemon check failed: $e');
    }
    if (mounted) setState(() => _daemonBusy = false);
  }

  Future<void> _batteryExemption() async {
    setState(() => _daemonBusy = true);
    final ok = await DaemonService.requestBatteryExemption();
    if (mounted) {
      setState(() {
        _daemonInfo = ok
            ? 'Battery exemption dialog opened (Android)'
            : 'Battery exemption unavailable (non-Android?)';
        _daemonBusy = false;
      });
    }
  }

  Future<void> _daemonsExtract() async {
    setState(() => _daemonBusy = true);
    final ok = await DaemonService.extractDaemons();
    if (mounted) {
      setState(() {
        _daemonInfo = ok
            ? 'Daemons extracted'
            : 'Extract failed (non-Android or missing assets)';
        _daemonBusy = false;
      });
    }
  }

  Future<void> _daemonsStart() async {
    setState(() => _daemonBusy = true);
    final shell = context.read<ShellService>();
    final ok = await DaemonService.startDaemons();
    if (mounted) {
      setState(() {
        _daemonInfo = ok ? 'Daemons started' : 'Start failed';
        _daemonBusy = false;
      });
      await _daemonsRefresh();
      // i2pd up now → transport may resolve to i2p; re-poll so the banner
      // flips online instead of waiting for the periodic poll.
      shell.refreshRelayStatus();
    }
  }

  Future<void> _daemonsStop() async {
    setState(() => _daemonBusy = true);
    final ok = await DaemonService.stopDaemons();
    if (mounted) {
      setState(() {
        _daemonInfo = ok ? 'Daemons stopped' : 'Stop failed';
        _daemonBusy = false;
      });
      await _daemonsRefresh();
    }
  }

  Future<void> _daemonPath(String name) async {
    final path = await DaemonService.getDaemonPath(name);
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('$name: ${path ?? 'not bundled'}')));
    }
  }

  Future<void> _ebpfBlock() async {
    final ip = _ebpfIp.text.trim();
    if (ip.isEmpty) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Enter an IP')));
      }
      return;
    }
    final ok = await context.read<EbpfService>().blockIp(ip);
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text(ok ? 'Blocked $ip' : 'Block failed (non-Linux?)')));
    }
  }

  Future<void> _ebpfUnblock() async {
    final ip = _ebpfIp.text.trim();
    if (ip.isEmpty) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Enter an IP')));
      }
      return;
    }
    final ok = await context.read<EbpfService>().unblockIp(ip);
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text(ok ? 'Unblocked $ip' : 'Unblock failed (non-Linux?)')));
    }
  }

  Future<void> _ebpfRefresh() async {
    try {
      await context.read<EbpfService>().refreshStats();
    } catch (e) {
      debugPrint('ebpf refresh: $e');
    }
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
        SnackBar(content: SelectableText('Connected to $url')),
      );
      _refresh();
    } else {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: SelectableText('Could not connect to that relay')),
      );
    }
  }

  Future<void> _removeRelay(String url) async {
    final service = context.read<NetworkService>();
    await service.removeRelay(url);
    _refresh();
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
              title: const Text('Freenet gateway (local port 7509)'),
            ),
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 4),
              child: Wrap(
                spacing: 8,
                children: [
                  for (final mode in TransportMode.values)
                    ChoiceChip(
                      label: Text(mode.label),
                      selected: service.transportMode == mode,
                      onSelected: (_) => _setTransportMode(mode),
                    ),
                ],
              ),
            ),
            if (service.resolved != null)
              ListTile(
                dense: true,
                leading: const Icon(Icons.route_outlined),
                title:
                    Text('In use: ${service.resolved!.resolved.toUpperCase()}'),
                subtitle: Text(service.resolved!.satisfied
                    ? 'Mode ${service.transportMode.label}'
                    : 'Preferred transport down - fell back to Nostr'),
                trailing: Icon(
                  service.resolved!.satisfied
                      ? Icons.check_circle
                      : Icons.warning_amber,
                  color: service.resolved!.satisfied
                      ? Colors.green
                      : Colors.orange,
                ),
              ),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
              child: Wrap(
                spacing: 8,
                children: [
                  TextButton.icon(
                    onPressed: service.meshRelayRunning
                        ? service.stopMeshRelay
                        : () => _startMeshRelay(service),
                    icon: Icon(service.meshRelayRunning
                        ? Icons.stop
                        : Icons.hub_outlined),
                    label: Text(service.meshRelayRunning
                        ? 'Stop mesh relay'
                        : 'Start mesh relay'),
                  ),
                  TextButton.icon(
                    onPressed: service.refreshMeshRelayStatus,
                    icon: const Icon(Icons.refresh),
                    label: const Text('Relay status'),
                  ),
                ],
              ),
            ),
            if (service.meshStatus != null)
              ListTile(
                dense: true,
                title: Text(
                    'Mesh relay: peers ${_meshPeersSummary(service.meshStatus!)}'),
                subtitle:
                    Text('published ${service.meshStatus!['published']} · '
                        'received ${service.meshStatus!['received']} · '
                        'delivered ${service.meshStatus!['delivered']}'),
              ),
            TextButton.icon(
              onPressed: () async {
                final messenger = ScaffoldMessenger.of(context);
                await context.read<NetworkService>().loadTransportMode();
                messenger.showSnackBar(SnackBar(
                    content:
                        Text('Saved mode: ${service.transportMode.name}')));
              },
              icon: const Icon(Icons.settings_backup_restore),
              label: const Text('Reload saved transport mode'),
            ),
            if (_i2pDest != null)
              ListTile(
                dense: true,
                title: const Text('I2P destination'),
                subtitle: SelectableText(_i2pDest!),
              ),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: Wrap(
                spacing: 8,
                children: [
                  TextButton.icon(
                    onPressed: _busy ? null : _startI2p,
                    icon: const Icon(Icons.play_arrow),
                    label: const Text('Start I2P session'),
                  ),
                  TextButton.icon(
                    onPressed: _stopI2p,
                    icon: const Icon(Icons.stop),
                    label: const Text('Stop I2P session'),
                  ),
                  TextButton.icon(
                    onPressed: _i2pStatus,
                    icon: const Icon(Icons.info_outline),
                    label: const Text('Session status'),
                  ),
                  TextButton.icon(
                    onPressed: _reinitRelays,
                    icon: const Icon(Icons.repeat),
                    label: const Text('Reconnect relays'),
                  ),
                ],
              ),
            ),
            const Divider(),
            Text('Bundled Daemons',
                style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 4),
            ListTile(
              dense: true,
              title: const Text('Available'),
              trailing: Text(_daemonsAvailable == null
                  ? 'unknown'
                  : _daemonsAvailable!
                      ? 'yes'
                      : 'no'),
            ),
            ListTile(
              dense: true,
              title: const Text('Background service'),
              subtitle: const Text('keeps daemons alive while app is closed '
                  '(Android foreground service)'),
              trailing: Text(_serviceRunning == null
                  ? 'unknown'
                  : _serviceRunning!
                      ? 'active'
                      : (PermissionsService.isAndroid
                          ? 'inactive'
                          : 'off (desktop)')),
            ),
            ListTile(
              dense: true,
              title: const Text('Battery exemption'),
              subtitle: const Text('stops OEM battery managers killing the '
                  'service (Android)'),
              trailing: TextButton(
                onPressed: _daemonBusy ? null : _batteryExemption,
                child: const Text('Request'),
              ),
            ),
            ListTile(
              dense: true,
              title: const Text('i2pd process'),
              trailing: Text(_i2pdRunning == null
                  ? 'unknown'
                  : _i2pdRunning!
                      ? 'running'
                      : 'stopped'),
            ),
            ListTile(
              dense: true,
              title: const Text('rnsd process'),
              trailing: Text(_rnsdRunning == null
                  ? 'unknown'
                  : _rnsdRunning!
                      ? 'running'
                      : 'stopped'),
            ),
            for (final entry in _daemonStatus.entries)
              if (entry.value is bool)
                ListTile(
                  dense: true,
                  title: Text(entry.key),
                  trailing: Text(entry.value as bool ? 'present' : 'missing'),
                ),
            if (_rnsdError != null)
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: ErrorStateText('rnsd: $_rnsdError'),
              ),
            if (_daemonInfo != null)
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: ErrorStateText(_daemonInfo!),
              ),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: Wrap(
                spacing: 8,
                children: [
                  TextButton.icon(
                    onPressed: _daemonBusy ? null : _daemonsRefresh,
                    icon: const Icon(Icons.refresh),
                    label: const Text('Check daemons'),
                  ),
                  TextButton.icon(
                    onPressed: _daemonBusy ? null : _daemonsExtract,
                    icon: const Icon(Icons.unarchive_outlined),
                    label: const Text('Extract'),
                  ),
                  TextButton.icon(
                    onPressed: _daemonBusy ? null : _daemonsStart,
                    icon: const Icon(Icons.play_arrow),
                    label: const Text('Start daemons'),
                  ),
                  TextButton.icon(
                    onPressed: _daemonBusy ? null : _daemonsStop,
                    icon: const Icon(Icons.stop),
                    label: const Text('Stop daemons'),
                  ),
                  TextButton.icon(
                    onPressed: _daemonBusy ? null : _showDaemonLogs,
                    icon: const Icon(Icons.receipt_long),
                    label: const Text('Logs'),
                  ),
                  TextButton.icon(
                    onPressed: () => _daemonPath(DaemonService.i2pd),
                    icon: const Icon(Icons.folder_open),
                    label: const Text('i2pd path'),
                  ),
                  TextButton.icon(
                    onPressed: () => _daemonPath(DaemonService.freenet),
                    icon: const Icon(Icons.folder_open),
                    label: const Text('freenet path'),
                  ),
                  TextButton.icon(
                    onPressed: () => _daemonPath(DaemonService.reticulum),
                    icon: const Icon(Icons.folder_open),
                    label: const Text('rnsd path'),
                  ),
                ],
              ),
            ),
            const Divider(),
            Text('eBPF firewall (Linux)',
                style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 4),
            Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: _ebpfIp,
                    decoration: const InputDecoration(
                      hintText: '1.2.3.4',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                    onSubmitted: (_) => _ebpfBlock(),
                  ),
                ),
                const SizedBox(width: 8),
                FilledButton(
                  onPressed: _ebpfBlock,
                  child: const Text('Block'),
                ),
                const SizedBox(width: 8),
                OutlinedButton(
                  onPressed: _ebpfUnblock,
                  child: const Text('Unblock'),
                ),
              ],
            ),
            const SizedBox(height: 8),
            Consumer<EbpfService>(
              builder: (context, ebpf, _) {
                return Column(
                  children: [
                    ListTile(
                      dense: true,
                      title: const Text('Shaper mode'),
                      trailing: Text(ebpf.mode),
                    ),
                    ListTile(
                      dense: true,
                      title: const Text('Dropped packets'),
                      trailing: Text('${ebpf.droppedPackets}'),
                    ),
                    ListTile(
                      dense: true,
                      title: const Text('Passed packets'),
                      trailing: Text('${ebpf.passedPackets}'),
                    ),
                    ListTile(
                      dense: true,
                      title: const Text('Nanos saved'),
                      trailing: Text('${ebpf.nanosSaved}'),
                    ),
                    ListTile(
                      dense: true,
                      title: const Text('Blocked peers'),
                      trailing: Text('${ebpf.blockedPeersCount}'),
                    ),
                    if (ebpf.lastError != null)
                      Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 16),
                        child: ErrorStateText(ebpf.lastError!),
                      ),
                  ],
                );
              },
            ),
            TextButton.icon(
              onPressed: _ebpfRefresh,
              icon: const Icon(Icons.memory),
              label: const Text('Refresh eBPF stats'),
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
                child: ErrorStateText('Error: $_error'),
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
                        ? '${relay.latencyMs} ms · connected ${_ago(relay.lastConnectAt)} ago'
                        : 'disconnected',
                  ),
                  trailing: IconButton(
                    icon: const Icon(Icons.delete_outline),
                    tooltip: 'Remove relay',
                    onPressed: () => _removeRelay(relay.url),
                  ),
                ),
            const Divider(),
            Text('Network Tools',
                style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 4),
            TextField(
              controller: _http3Url,
              decoration: const InputDecoration(
                labelText: 'HTTP/3 fetch URL',
                border: OutlineInputBorder(),
                isDense: true,
              ),
              onSubmitted: (_) => _http3Fetch(),
            ),
            const SizedBox(height: 8),
            Row(
              children: [
                FilledButton.icon(
                  onPressed: _busy ? null : _http3Fetch,
                  icon: const Icon(Icons.cloud_download_outlined),
                  label: const Text('Fetch over HTTP/3'),
                ),
                const SizedBox(width: 8),
                OutlinedButton.icon(
                  onPressed: _notifyInterfaceChange,
                  icon: const Icon(Icons.wifi),
                  label: const Text('Notify interface change'),
                ),
              ],
            ),
            if (_http3Info != null)
              ListTile(
                dense: true,
                title: SelectableText(_http3Info!),
              ),
            const Divider(),
            Text('Prolly Tree Reconcile',
                style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 4),
            TextField(
              controller: _localKvField,
              decoration: const InputDecoration(
                labelText: 'Local KV (JSON object)',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: _remoteHashField,
              decoration: const InputDecoration(
                labelText: 'Remote root hash',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            FilledButton.icon(
              onPressed: _busy ? null : _reconcile,
              icon: const Icon(Icons.merge_type),
              label: const Text('Reconcile with peer'),
            ),
            if (_reconcileInfo != null)
              ListTile(
                dense: true,
                title: SelectableText(_reconcileInfo!),
              ),
            const Divider(),
            Text('Freenet (external infra)',
                style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 4),
            TextField(
              controller: _freenetUrl,
              decoration: const InputDecoration(
                labelText: 'Gateway URL',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: _freenetAuthToken,
              decoration: const InputDecoration(
                labelText: 'Auth token',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: _freenetKey,
              decoration: const InputDecoration(
                labelText: 'Contract key',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: _freenetStateField,
              decoration: const InputDecoration(
                labelText: 'Contract state (JSON)',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: _freenetSummaryField,
              decoration: const InputDecoration(
                labelText: 'Subscribe summary (JSON)',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            Wrap(
              spacing: 8,
              children: [
                FilledButton.icon(
                  onPressed: _busy ? null : _freenetConnect,
                  icon: const Icon(Icons.link),
                  label: const Text('Freenet: connect'),
                ),
                TextButton.icon(
                  onPressed: _busy ? null : _freenetGetContract,
                  icon: const Icon(Icons.download),
                  label: const Text('Get contract'),
                ),
                TextButton.icon(
                  onPressed: _busy ? null : _freenetPutContract,
                  icon: const Icon(Icons.upload),
                  label: const Text('Put contract'),
                ),
                TextButton.icon(
                  onPressed: _busy ? null : _freenetSubscribe,
                  icon: const Icon(Icons.subscriptions_outlined),
                  label: const Text('Subscribe'),
                ),
              ],
            ),
            if (_freenetInfo != null)
              ListTile(
                dense: true,
                title: SelectableText(_freenetInfo!),
              ),
            const Divider(),
            Text('I2P SAM (raw)',
                style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 4),
            Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: _samHost,
                    decoration: const InputDecoration(
                      labelText: 'SAM host',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                SizedBox(
                  width: 80,
                  child: TextField(
                    controller: _samPort,
                    decoration: const InputDecoration(
                      labelText: 'Port',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 8),
            TextField(
              controller: _sessionId,
              decoration: const InputDecoration(
                labelText: 'Session id',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: _samDestField,
              decoration: const InputDecoration(
                labelText: 'Destination',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            Wrap(
              spacing: 8,
              children: [
                FilledButton.icon(
                  onPressed: _samConnect,
                  icon: const Icon(Icons.link),
                  label: const Text('SAM connect'),
                ),
                TextButton.icon(
                  onPressed: _samCreateSession,
                  icon: const Icon(Icons.add_circle_outline),
                  label: const Text('Create session'),
                ),
                TextButton.icon(
                  onPressed: _samGenerateDestination,
                  icon: const Icon(Icons.fingerprint),
                  label: const Text('Generate destination'),
                ),
                TextButton.icon(
                  onPressed: _samConnectToDestination,
                  icon: const Icon(Icons.near_me_outlined),
                  label: const Text('Connect to destination'),
                ),
              ],
            ),
            if (_samInfo != null)
              ListTile(
                dense: true,
                title: SelectableText(_samInfo!),
              ),
            const Divider(),
            Text('Raw Relay Ops',
                style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 4),
            TextField(
              controller: _relayFilterField,
              decoration: const InputDecoration(
                labelText: 'Filter (JSON)',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: _subscriptionIdField,
              decoration: const InputDecoration(
                labelText: 'Subscription id',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: _eventJsonField,
              maxLines: 2,
              decoration: const InputDecoration(
                labelText: 'Event (JSON)',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            Wrap(
              spacing: 8,
              children: [
                FilledButton.icon(
                  onPressed: _busy ? null : _relaySubscribe,
                  icon: const Icon(Icons.subscriptions_outlined),
                  label: const Text('Subscribe'),
                ),
                TextButton.icon(
                  onPressed: _busy ? null : _relayUnsubscribe,
                  icon: const Icon(Icons.unsubscribe_outlined),
                  label: const Text('Unsubscribe'),
                ),
                TextButton.icon(
                  onPressed: _busy ? null : _relayPublishEvent,
                  icon: const Icon(Icons.send_outlined),
                  label: const Text('Publish event'),
                ),
                TextButton.icon(
                  onPressed: _busy ? null : _relayQueryEvents,
                  icon: const Icon(Icons.search),
                  label: const Text('Query events'),
                ),
              ],
            ),
            if (_rawRelayInfo != null)
              ListTile(
                dense: true,
                title: SelectableText(_rawRelayInfo!),
              ),
            const Divider(),
            Text('Experimental',
                style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 4),
            TextField(
              controller: _bleBeaconField,
              maxLines: 2,
              decoration: const InputDecoration(
                labelText: 'BLE beacon (JSON)',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: _bleRootField,
              decoration: const InputDecoration(
                labelText: 'Local root hash (hex)',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            TextButton.icon(
              onPressed: _busy ? null : _processBleBeacon,
              icon: const Icon(Icons.bluetooth),
              label: const Text('Process beacon (experimental)'),
            ),
            if (_bleInfo != null)
              ListTile(
                dense: true,
                title: SelectableText(_bleInfo!),
              ),
            const SizedBox(height: 8),
            TextField(
              controller: _zkProofField,
              maxLines: 2,
              decoration: const InputDecoration(
                labelText: 'ZK WoT proof (JSON)',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: _zkRootField,
              decoration: const InputDecoration(
                labelText: 'Expected WoT root (hex)',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: _zkNullifiersField,
              decoration: const InputDecoration(
                labelText: 'Blacklisted nullifiers (JSON)',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            TextButton.icon(
              onPressed: _busy ? null : _verifyZkWot,
              icon: const Icon(Icons.verified_outlined),
              label: const Text('Verify ZK WoT proof (experimental)'),
            ),
            if (_zkInfo != null)
              ListTile(
                dense: true,
                title: SelectableText(_zkInfo!),
              ),
          ],
        ),
      ),
    );
  }

  @override
  void dispose() {
    _relayField.dispose();
    _ebpfIp.dispose();
    _http3Url.dispose();
    _localKvField.dispose();
    _remoteHashField.dispose();
    _freenetUrl.dispose();
    _freenetAuthToken.clear();
    _freenetAuthToken.dispose();
    _freenetKey.dispose();
    _freenetStateField.dispose();
    _freenetSummaryField.dispose();
    _samHost.dispose();
    _samPort.dispose();
    _sessionId.dispose();
    _samDestField.dispose();
    _relayFilterField.dispose();
    _subscriptionIdField.dispose();
    _eventJsonField.dispose();
    _bleBeaconField.dispose();
    _bleRootField.dispose();
    _zkProofField.dispose();
    _zkRootField.dispose();
    _zkNullifiersField.dispose();
    super.dispose();
  }

  String _ago(int ts) => ts <= 0 ? 'never' : relativeTime(ts);
}
