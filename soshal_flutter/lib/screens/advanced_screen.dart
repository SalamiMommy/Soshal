import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/auth_service.dart';
import '../services/crypto_service.dart';
import '../services/error_log.dart';
import '../services/feed_service.dart';
import '../services/mesh_service.dart';
import '../services/network_service.dart';
import '../services/session_service.dart';
import '../services/settings_service.dart';
import '../services/signer_service.dart';
import '../services/sync_service.dart';
import '../services/telemetry_service.dart';
import '../widgets/settings_scaffold.dart';

/// Advanced settings — relay flags (read/write toggles), diagnostics, and
/// transport probes. Also hosts the wired-in diagnostics cluster: sync
/// engine controls, storage/db info, ephemeral cleanup, crypto utilities and
/// the telemetry transcript viewer.
class AdvancedScreen extends StatefulWidget {
  const AdvancedScreen({super.key});

  @override
  State<AdvancedScreen> createState() => _AdvancedScreenState();
}

class _AdvancedScreenState extends State<AdvancedScreen> {
  Map<String, dynamic>? _diagnostics;
  bool _i2p = false;
  bool _freenet = false;
  String? _diagError;
  String? _dbPath;
  String? _engineMode;
  bool _syncRunning = false;
  String _outboxSummary = '{}';
  String? _ephemeralResult;
  final _hashController = TextEditingController();
  String? _hashResult;
  final _b64Controller = TextEditingController();
  String? _b64Result;
  String? _telemetryInfo;
  String? _telemetryTranscript;
  bool _telemetrySealed = false;
  CryptoService get _crypto => context.read<CryptoService>();
  final _nsecController = TextEditingController();
  String? _meshResult;
  final _shaController = TextEditingController();
  String? _shaResult;
  final _hmacKeyController = TextEditingController();
  final _hmacMsgController = TextEditingController();
  String? _hmacResult;
  final _hkdfController = TextEditingController();
  String? _hkdfResult;
  final _randController = TextEditingController();
  String? _randResult;
  final _zeroizeController = TextEditingController();
  String? _zeroizeResult;
  final _nip44TextController = TextEditingController();
  final _nip44PubkeyController = TextEditingController();
  String? _nip44Result;
  String? _pqcPk;
  String? _pqcSk;
  String? _pqcCt;
  String? _pqcSs;
  String? _pqcDecapsSs;
  String? _frostSharesJson;
  String? _frostGroupPubkey;
  String? _frostResult;
  final _frostMsgController = TextEditingController();
  String? _pirQueryJson;
  String? _pirResult;
  final _sqlController = TextEditingController();
  String? _sqlResult;
  String? _zkResult;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final networkService = context.read<NetworkService>();
    try {
      final diag = networkService.fetchSysDiagnostics();
      _diagnostics = jsonDecode(diag) as Map<String, dynamic>?;
      _diagError = null;
    } catch (e) {
      _diagError = '$e';
    }
    try {
      await networkService.refresh();
      _i2p = networkService.i2p ?? false;
      _freenet = networkService.freenet ?? false;
    } catch (e, st) {
      debugPrint('network refresh failed: $e');
      logRuntimeError('network refresh: $e', st);
    }
    if (!mounted) return;
    final settings = context.read<SettingsService>();
    final sync = context.read<SyncService>();
    final telemetry = context.read<TelemetryService>();
    try {
      _dbPath = settings.dbPath();
      _engineMode = settings.ioEngineMode();
    } catch (e) {
      _dbPath = 'unavailable: $e';
    }
    _syncRunning = sync.syncRunning();
    _outboxSummary = sync.outboxSummary();
    _telemetryInfo = telemetry.infoJson();
    _telemetryTranscript = telemetry.readAllJson();
    _telemetrySealed = telemetry.sealed;
    if (mounted) setState(() {});
  }

  Future<void> _runBackgroundSync() async {
    final sync = context.read<SyncService>();
    final snack = ScaffoldMessenger.of(context);
    final count = await sync.runBackgroundSync();
    if (!mounted) return;
    setState(() {
      _syncRunning = sync.syncRunning();
      _outboxSummary = sync.outboxSummary();
    });
    snack.showSnackBar(SnackBar(
        content: Text(count >= 0
            ? 'Sync pass done ($count events)'
            : 'Sync pass failed — see logs')));
  }

  Future<void> _runEpochGc() async {
    final sync = context.read<SyncService>();
    final snack = ScaffoldMessenger.of(context);
    await sync.runScheduledEpochGc();
    if (!mounted) return;
    snack.showSnackBar(SnackBar(
        content: SelectableText('Epoch GC: ${sync.lastError ?? 'ok'}')));
  }

  Future<void> _stopSync() async {
    final sync = context.read<SyncService>();
    await sync.stop();
    if (!mounted) return;
    setState(() => _syncRunning = sync.syncRunning());
  }

  void _cleanEphemeral() {
    final settings = context.read<SettingsService>();
    final snack = ScaffoldMessenger.of(context);
    try {
      final expired = settings.cleanExpiredEphemeral();
      setState(() => _ephemeralResult = 'Expired: ${expired.length}');
      snack.showSnackBar(SnackBar(
          content: SelectableText('Cleaned ${expired.length} expired items')));
    } catch (e) {
      setState(() => _ephemeralResult = 'Failed: $e');
    }
  }

  void _hashText() {
    final settings = context.read<SettingsService>();
    setState(() {
      _hashResult = _hashController.text.isEmpty
          ? null
          : settings.sha256Hex(_hashController.text);
    });
  }

  void _b64Encode() {
    final settings = context.read<SettingsService>();
    setState(() {
      _b64Result = _b64Controller.text.isEmpty
          ? null
          : settings.b64UrlEncode(_b64Controller.text);
    });
  }

  void _b64Decode() {
    final settings = context.read<SettingsService>();
    setState(() {
      _b64Result = _b64Controller.text.isEmpty
          ? null
          : settings.b64UrlDecode(_b64Controller.text);
    });
  }

  void _dumpTelemetry() {
    final telemetry = context.read<TelemetryService>();
    final snack = ScaffoldMessenger.of(context);
    final dump = telemetry.dumpEncrypted();
    if (!mounted) return;
    setState(() => _telemetrySealed = telemetry.sealed);
    snack.showSnackBar(SnackBar(
        content: Text(dump == null
            ? 'Telemetry dump failed'
            : 'Dump saved (${dump.length} bytes, sealed: ${telemetry.sealed})')));
  }

  void _recordTelemetryEvent() {
    context.read<TelemetryService>().record('ui-debug-event');
  }

  /// First 24 hex chars + ellipsis (key material is never shown in full).
  static String prefixHex(String hex) =>
      hex.length > 24 ? '${hex.substring(0, 24)}…' : hex;

  void _cryptoSha() {
    try {
      setState(() {
        _shaResult = _shaController.text.isEmpty
            ? null
            : _crypto.sha256Hex(_shaController.text);
      });
    } catch (e) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: SelectableText('sha256: $e')));
    }
  }

  void _cryptoShaBytes() {
    try {
      final text = _shaController.text.trim();
      if (text.isEmpty) {
        setState(() => _shaResult = null);
        return;
      }
      setState(() {
        _shaResult = 'utf8: ${_crypto.sha256Hex(text)}\n'
            'bytes: ${_crypto.sha256Bytes(text)}';
      });
    } catch (e) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: SelectableText('sha256 bytes: $e')));
    }
  }

  void _threadAffinity(bool performance) {
    try {
      final ok = _crypto.applyThreadAffinity(performance);
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text(ok
              ? 'Pinned to ${performance ? 'performance' : 'efficiency'} cores'
              : 'Thread affinity unavailable')));
    } catch (e) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: SelectableText('affinity: $e')));
    }
  }

  void _cryptoHmac() {
    try {
      setState(() {
        _hmacResult =
            _hmacKeyController.text.isEmpty || _hmacMsgController.text.isEmpty
                ? null
                : _crypto.hmacSha256(
                    _hmacKeyController.text, _hmacMsgController.text);
      });
    } catch (e) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: SelectableText('hmac: $e')));
    }
  }

  void _cryptoHkdf() {
    try {
      setState(() {
        _hkdfResult = _hkdfController.text.isEmpty
            ? null
            : _crypto.hkdfExpand(_hkdfController.text);
      });
    } catch (e) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: SelectableText('hkdf: $e')));
    }
  }

  void _cryptoRandom() {
    try {
      final len = int.tryParse(_randController.text.trim());
      setState(() {
        if (len == null || len <= 0) {
          _randResult = 'count must be a positive integer';
          return;
        }
        final hex = _crypto.randomBytes(len);
        _randResult = '$len bytes (${hex.length ~/ 2}):\n$hex';
      });
    } catch (e) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: SelectableText('random bytes: $e')));
    }
  }

  void _cryptoZeroize() {
    try {
      final ok = _crypto.zeroize(_zeroizeController.text);
      setState(() => _zeroizeResult = ok ? 'zeroized ok' : 'zeroize failed');
    } catch (e) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: SelectableText('zeroize: $e')));
    }
  }

  Future<void> _nip44RoundTrip() async {
    final signer = context.read<SignerService>();
    final snack = ScaffoldMessenger.of(context);
    var pubkey = _nip44PubkeyController.text.trim();
    if (pubkey.isEmpty) {
      try {
        pubkey = await signer.pubkey();
      } catch (_) {}
    }
    if (_nip44TextController.text.trim().isEmpty || pubkey.isEmpty) {
      snack.showSnackBar(SnackBar(
          content: SelectableText('Fill plaintext (pubkey optional)')));
      return;
    }
    try {
      final ct =
          await signer.nip44Encrypt(_nip44TextController.text.trim(), pubkey);
      final pt = await signer.nip44Decrypt(ct, pubkey);
      final ct2 =
          _crypto.nip44Encrypt(_nip44TextController.text.trim(), pubkey);
      final pt2 = _crypto.nip44Decrypt(ct2, pubkey);
      if (!mounted) return;
      setState(() {
        _nip44Result = 'ciphertext:\n$ct\n\ndecrypted:\n$pt\n\n'
            'match: ${pt == _nip44TextController.text.trim()}\n\n'
            'via crypto alias match: ${pt2 == _nip44TextController.text.trim()}';
      });
    } catch (e) {
      snack.showSnackBar(SnackBar(content: SelectableText('nip44: $e')));
    }
  }

  Future<void> _pqcKeygen() async {
    final snack = ScaffoldMessenger.of(context);
    try {
      final res =
          jsonDecode(await _crypto.pqcKemKeygen()) as Map<String, dynamic>;
      if (!mounted) return;
      setState(() {
        _pqcPk = res['pk'] as String?;
        _pqcSk = res['sk'] as String?;
        _pqcCt = null;
        _pqcSs = null;
        _pqcDecapsSs = null;
      });
    } catch (e) {
      snack.showSnackBar(SnackBar(content: SelectableText('pqc keygen: $e')));
    }
  }

  Future<void> _pqcEncaps() async {
    final snack = ScaffoldMessenger.of(context);
    final pk = _pqcPk;
    if (pk == null) {
      snack.showSnackBar(SnackBar(content: SelectableText('Run keygen first')));
      return;
    }
    try {
      final res =
          jsonDecode(await _crypto.pqcKemEncaps(pk)) as Map<String, dynamic>;
      if (!mounted) return;
      setState(() {
        _pqcCt = res['ct'] as String?;
        _pqcSs = res['ss'] as String?;
        _pqcDecapsSs = null;
      });
    } catch (e) {
      snack.showSnackBar(SnackBar(content: SelectableText('pqc encaps: $e')));
    }
  }

  Future<void> _pqcDecaps() async {
    final snack = ScaffoldMessenger.of(context);
    final ct = _pqcCt;
    final sk = _pqcSk;
    if (ct == null || sk == null) {
      snack.showSnackBar(SnackBar(content: SelectableText('Run encaps first')));
      return;
    }
    try {
      final ss = await _crypto.pqcKemDecaps(ct, sk);
      if (!mounted) return;
      setState(() => _pqcDecapsSs = ss);
    } catch (e) {
      snack.showSnackBar(SnackBar(content: SelectableText('pqc decaps: $e')));
    }
  }

  Future<void> _frostKeygen() async {
    final snack = ScaffoldMessenger.of(context);
    try {
      final pk = await context.read<SignerService>().pubkey();
      final shares = await _crypto.frostGenerateJuryKeys(
          threshold: 2, totalParticipants: 3, groupPubkey: pk);
      if (!mounted) return;
      setState(() {
        _frostSharesJson = shares;
        _frostGroupPubkey = pk;
        _frostResult = null;
      });
    } catch (e) {
      snack.showSnackBar(SnackBar(content: SelectableText('frost keygen: $e')));
    }
  }

  Future<void> _frostAggregate() async {
    final snack = ScaffoldMessenger.of(context);
    final sharesJson = _frostSharesJson;
    final groupPubkey = _frostGroupPubkey;
    if (sharesJson == null || groupPubkey == null) {
      snack.showSnackBar(SnackBar(content: SelectableText('Run keygen first')));
      return;
    }
    try {
      final msgHex = _crypto.sha256Hex(_frostMsgController.text.trim());
      final keys = jsonDecode(sharesJson) as List<dynamic>;
      final sigShares = [
        for (final k in keys.take(2))
          {
            'participant_id': k['participant_id'],
            'sig_share_hex': 'sig_share_${k['participant_id']}_$msgHex',
          },
      ];
      final res = await _crypto.frostAggregateSignature(
        sharesJson: jsonEncode(sigShares),
        threshold: 2,
        groupPubkey: groupPubkey,
        messageHex: msgHex,
      );
      if (!mounted) return;
      setState(() => _frostResult = res);
    } catch (e) {
      snack.showSnackBar(
          SnackBar(content: SelectableText('frost aggregate: $e')));
    }
  }

  Future<void> _pirGenerate() async {
    final snack = ScaffoldMessenger.of(context);
    try {
      final pk = await context.read<SignerService>().pubkey();
      final query = await _crypto.pirGenerateQuery(
          targetIndex: 1, dimension: 4, clientPubkey: pk);
      if (!mounted) return;
      setState(() {
        _pirQueryJson = query;
        _pirResult = null;
      });
    } catch (e) {
      snack.showSnackBar(SnackBar(content: SelectableText('pir generate: $e')));
    }
  }

  Future<void> _pirEvaluate() async {
    final snack = ScaffoldMessenger.of(context);
    final query = _pirQueryJson;
    if (query == null) {
      snack.showSnackBar(SnackBar(content: SelectableText('Run query first')));
      return;
    }
    try {
      final res = await _crypto.pirEvaluateQuery(
          queryJson: query, recordHexList: const ['cafebabe', 'deadbeef']);
      if (!mounted) return;
      setState(() => _pirResult = res);
    } catch (e) {
      snack.showSnackBar(SnackBar(content: SelectableText('pir evaluate: $e')));
    }
  }

  Future<void> _dbPurge(int cutoffSecs) async {
    final snack = ScaffoldMessenger.of(context);
    try {
      final n = await context.read<FeedService>().dbDeleteOlderThan(cutoffSecs);
      if (!mounted) return;
      setState(() => _meshResult = 'posts removed: $n');
    } catch (e) {
      snack.showSnackBar(SnackBar(content: SelectableText('db purge: $e')));
    }
  }

  Future<void> _dbPurgeAll() async {
    final snack = ScaffoldMessenger.of(context);
    try {
      final n = await context.read<FeedService>().dbDeleteAllPosts();
      if (!mounted) return;
      setState(() => _meshResult = 'all posts removed: $n');
    } catch (e) {
      snack.showSnackBar(SnackBar(content: SelectableText('db purge all: $e')));
    }
  }

  Future<void> _purgeGeohashPeers() async {
    final snack = ScaffoldMessenger.of(context);
    try {
      final n =
          context.read<SettingsService>().purgeStaleGeohashPeers(7 * 24 * 3600);
      if (!mounted) return;
      setState(() => _meshResult = 'geohash peers purged: $n');
    } catch (e) {
      snack
          .showSnackBar(SnackBar(content: SelectableText('geohash purge: $e')));
    }
  }

  Future<void> _pruneMeshLinks() async {
    final snack = ScaffoldMessenger.of(context);
    try {
      final n = await context.read<MeshService>().pruneStaleLinks();
      if (!mounted) return;
      setState(() => _meshResult = 'stale links pruned: $n');
    } catch (e) {
      snack.showSnackBar(SnackBar(content: SelectableText('mesh prune: $e')));
    }
  }

  Future<void> _pruneMeshRoutes() async {
    final snack = ScaffoldMessenger.of(context);
    try {
      final n = await context
          .read<MeshService>()
          .pruneRoutes(DateTime.now().millisecondsSinceEpoch ~/ 1000);
      if (!mounted) return;
      setState(() => _meshResult = 'expired routes pruned: $n');
    } catch (e) {
      snack.showSnackBar(SnackBar(content: SelectableText('mesh routes: $e')));
    }
  }

  Future<void> _meshNodeId() async {
    final snack = ScaffoldMessenger.of(context);
    try {
      final pubkey = context.read<SessionService>().activePubkey ?? '';
      final id = await context
          .read<MeshService>()
          .skademliaNodeId(pubkey: pubkey, staticNonce: 0, dynamicNonce: 0);
      if (!mounted) return;
      setState(
          () => _meshResult = 'node id: ${id ?? 'PoW miss (try nonce>0)'}');
    } catch (e) {
      snack.showSnackBar(SnackBar(content: SelectableText('skademlia: $e')));
    }
  }

  Future<void> _signerFromNsec() async {
    if (!kDebugMode) return;
    final snack = ScaffoldMessenger.of(context);
    final nsec = _nsecController.text.trim();
    if (nsec.isEmpty) {
      snack.showSnackBar(SnackBar(content: SelectableText('Enter an nsec')));
      return;
    }
    try {
      final pubkey =
          await context.read<AuthService>().inProcessSignerPubkey(nsec);
      if (!mounted) return;
      setState(() => _meshResult = 'derived pubkey: $pubkey');
    } catch (e) {
      snack.showSnackBar(SnackBar(content: SelectableText('signer probe: $e')));
    }
  }

  void _sqlRun() {
    final sql = _sqlController.text.trim();
    if (sql.isEmpty) return;
    try {
      final rows = _crypto.dbQueryRaw(sql);
      String pretty;
      try {
        pretty = const JsonEncoder.withIndent('  ').convert(jsonDecode(rows));
      } catch (_) {
        pretty = rows;
      }
      setState(() => _sqlResult = pretty);
    } catch (e) {
      setState(() => _sqlResult = 'Error: $e');
    }
  }

  Future<void> _applyZkRollup() async {
    final sync = context.read<SyncService>();
    final snack = ScaffoldMessenger.of(context);
    final ok = await sync.applyRollup(sync.outboxSummary());
    if (!mounted) return;
    setState(
        () => _zkResult = ok ? 'rollup applied' : 'failed — ${sync.lastError}');
    snack.showSnackBar(SnackBar(
        content:
            SelectableText(ok ? 'ZK rollup applied' : 'ZK rollup failed')));
  }

  @override
  void dispose() {
    _hashController.dispose();
    _b64Controller.dispose();
    _shaController.dispose();
    _hmacKeyController.dispose();
    _hmacMsgController.dispose();
    _hkdfController.dispose();
    _randController.dispose();
    _zeroizeController.dispose();
    _nip44TextController.dispose();
    _nip44PubkeyController.dispose();
    _frostMsgController.dispose();
    _sqlController.dispose();
    _nsecController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return SettingsScaffold(
      title: 'Advanced',
      padding: const EdgeInsets.all(16),
      children: [
        Text('Transports', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 4),
        ListTile(
          leading: Icon(
            _i2p ? Icons.check_circle : Icons.circle_outlined,
            color: _i2p ? Colors.green : null,
          ),
          title: const Text('I2P tunnel (local i2pd SOCKS 7656)'),
        ),
        ListTile(
          leading: Icon(
            _freenet ? Icons.check_circle : Icons.circle_outlined,
            color: _freenet ? Colors.green : null,
          ),
          title: const Text('Freenet gateway (local port 8888)'),
        ),
        const Divider(),
        Text('Sync engine', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 4),
        ListTile(
          leading: Icon(
            _syncRunning ? Icons.sync : Icons.sync_disabled,
            color: _syncRunning ? Colors.green : null,
          ),
          title: Text(_syncRunning ? 'Running' : 'Stopped'),
          subtitle: Text('Outbox: $_outboxSummary'),
        ),
        Row(
          children: [
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _runBackgroundSync,
                icon: const Icon(Icons.sync),
                label: const Text('Run sync pass'),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _runEpochGc,
                icon: const Icon(Icons.cleaning_services),
                label: const Text('Epoch GC'),
              ),
            ),
          ],
        ),
        const SizedBox(height: 8),
        OutlinedButton.icon(
          onPressed: _syncRunning ? _stopSync : null,
          icon: const Icon(Icons.stop),
          label: const Text('Stop engine'),
        ),
        const Divider(),
        Text('Storage & DB', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 4),
        ListTile(
          leading: const Icon(Icons.storage),
          title: const Text('Engine mode'),
          subtitle: Text(_engineMode ?? 'unknown'),
        ),
        ListTile(
          leading: const Icon(Icons.folder),
          title: const Text('Database path'),
          subtitle: SelectableText(_dbPath ?? 'unavailable'),
        ),
        const Divider(),
        Text('Ephemeral media', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 4),
        OutlinedButton.icon(
          onPressed: _cleanEphemeral,
          icon: const Icon(Icons.delete_sweep),
          label: const Text('Clean expired'),
        ),
        if (_ephemeralResult != null) ...[
          const SizedBox(height: 4),
          Text(_ephemeralResult!, style: const TextStyle(fontSize: 12)),
        ],
        const Divider(),
        Text('Utilities', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 4),
        TextField(
          controller: _hashController,
          decoration: const InputDecoration(
            labelText: 'Text to hash (SHA-256)',
            border: OutlineInputBorder(),
            isDense: true,
          ),
          onSubmitted: (_) => _hashText(),
        ),
        const SizedBox(height: 4),
        OutlinedButton.icon(
          onPressed: _hashText,
          icon: const Icon(Icons.tag),
          label: const Text('Hash'),
        ),
        if (_hashResult != null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: SelectableText(_hashResult!,
                style: const TextStyle(fontSize: 12)),
          ),
        const SizedBox(height: 12),
        TextField(
          controller: _b64Controller,
          decoration: const InputDecoration(
            labelText: 'Base64url text',
            border: OutlineInputBorder(),
            isDense: true,
          ),
        ),
        const SizedBox(height: 4),
        Row(
          children: [
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _b64Encode,
                icon: const Icon(Icons.arrow_downward),
                label: const Text('Encode'),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _b64Decode,
                icon: const Icon(Icons.arrow_upward),
                label: const Text('Decode'),
              ),
            ),
          ],
        ),
        if (_b64Result != null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: SelectableText(_b64Result!,
                style: const TextStyle(fontSize: 12)),
          ),
        const SizedBox(height: 12),
        Row(
          children: [
            Expanded(
              child: OutlinedButton.icon(
                onPressed: () => _threadAffinity(true),
                icon: const Icon(Icons.speed),
                label: const Text('Pin perf cores'),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: OutlinedButton.icon(
                onPressed: () => _threadAffinity(false),
                icon: const Icon(Icons.energy_savings_leaf),
                label: const Text('Pin efficiency'),
              ),
            ),
          ],
        ),
        const Divider(),
        Text('Crypto', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 4),
        TextField(
          controller: _shaController,
          decoration: const InputDecoration(
            labelText: 'SHA-256 input',
            border: OutlineInputBorder(),
            isDense: true,
          ),
          onSubmitted: (_) => _cryptoSha(),
        ),
        const SizedBox(height: 4),
        OutlinedButton.icon(
          onPressed: _cryptoSha,
          icon: const Icon(Icons.tag),
          label: const Text('SHA-256'),
        ),
        const SizedBox(height: 4),
        OutlinedButton.icon(
          onPressed: _cryptoShaBytes,
          icon: const Icon(Icons.tag),
          label: const Text('SHA-256 (utf8 + bytes)'),
        ),
        if (_shaResult != null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: SelectableText(_shaResult!,
                style: const TextStyle(fontSize: 12)),
          ),
        const SizedBox(height: 12),
        TextField(
          controller: _hmacKeyController,
          decoration: const InputDecoration(
            labelText: 'HMAC key',
            border: OutlineInputBorder(),
            isDense: true,
          ),
        ),
        const SizedBox(height: 4),
        TextField(
          controller: _hmacMsgController,
          decoration: const InputDecoration(
            labelText: 'HMAC message',
            border: OutlineInputBorder(),
            isDense: true,
          ),
        ),
        const SizedBox(height: 4),
        OutlinedButton.icon(
          onPressed: _cryptoHmac,
          icon: const Icon(Icons.key),
          label: const Text('HMAC-SHA256'),
        ),
        if (_hmacResult != null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: SelectableText(_hmacResult!,
                style: const TextStyle(fontSize: 12)),
          ),
        const SizedBox(height: 12),
        TextField(
          controller: _hkdfController,
          decoration: const InputDecoration(
            labelText: 'HKDF expand (ikm → 32 bytes)',
            border: OutlineInputBorder(),
            isDense: true,
          ),
          onSubmitted: (_) => _cryptoHkdf(),
        ),
        const SizedBox(height: 4),
        OutlinedButton.icon(
          onPressed: _cryptoHkdf,
          icon: const Icon(Icons.unfold_more),
          label: const Text('HKDF expand'),
        ),
        if (_hkdfResult != null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: SelectableText(_hkdfResult!,
                style: const TextStyle(fontSize: 12)),
          ),
        const SizedBox(height: 12),
        TextField(
          controller: _randController,
          decoration: const InputDecoration(
            labelText: 'Random bytes count (1..=65536)',
            border: OutlineInputBorder(),
            isDense: true,
          ),
          onSubmitted: (_) => _cryptoRandom(),
        ),
        const SizedBox(height: 4),
        OutlinedButton.icon(
          onPressed: _cryptoRandom,
          icon: const Icon(Icons.casino),
          label: const Text('Random bytes'),
        ),
        if (_randResult != null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: SelectableText(_randResult!,
                style: const TextStyle(fontSize: 12)),
          ),
        const SizedBox(height: 12),
        TextField(
          controller: _zeroizeController,
          decoration: const InputDecoration(
            labelText: 'Zeroize demo input',
            border: OutlineInputBorder(),
            isDense: true,
          ),
          onSubmitted: (_) => _cryptoZeroize(),
        ),
        const SizedBox(height: 4),
        OutlinedButton.icon(
          onPressed: _cryptoZeroize,
          icon: const Icon(Icons.cleaning_services),
          label: const Text('Zeroize'),
        ),
        if (_zeroizeResult != null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: SelectableText(_zeroizeResult!,
                style: const TextStyle(fontSize: 12)),
          ),
        const SizedBox(height: 12),
        TextField(
          controller: _nip44TextController,
          decoration: const InputDecoration(
            labelText: 'NIP-44 plaintext',
            border: OutlineInputBorder(),
            isDense: true,
          ),
        ),
        const SizedBox(height: 4),
        TextField(
          controller: _nip44PubkeyController,
          decoration: const InputDecoration(
            labelText: 'NIP-44 recipient pubkey (empty = self)',
            border: OutlineInputBorder(),
            isDense: true,
          ),
        ),
        const SizedBox(height: 4),
        OutlinedButton.icon(
          onPressed: _nip44RoundTrip,
          icon: const Icon(Icons.lock_reset),
          label: const Text('NIP-44 encrypt → decrypt'),
        ),
        if (_nip44Result != null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: SelectableText(_nip44Result!,
                style: const TextStyle(fontSize: 12)),
          ),
        const Divider(),
        Text('PQC KEM (experimental)',
            style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 4),
        Row(
          children: [
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _pqcKeygen,
                icon: const Icon(Icons.vpn_key),
                label: const Text('Keygen'),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _pqcEncaps,
                icon: const Icon(Icons.lock),
                label: const Text('Encaps'),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _pqcDecaps,
                icon: const Icon(Icons.lock_open),
                label: const Text('Decaps'),
              ),
            ),
          ],
        ),
        if (_pqcPk != null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: SelectableText('pk: ${prefixHex(_pqcPk!)}',
                style: const TextStyle(fontSize: 12)),
          ),
        if (_pqcCt != null)
          Padding(
            padding: const EdgeInsets.only(top: 2),
            child: SelectableText('ct: ${prefixHex(_pqcCt!)}',
                style: const TextStyle(fontSize: 12)),
          ),
        if (_pqcSs != null && _pqcDecapsSs != null)
          Padding(
            padding: const EdgeInsets.only(top: 2),
            child: SelectableText(
              'ss match: ${_pqcSs == _pqcDecapsSs} '
              '(${prefixHex(_pqcDecapsSs!)})',
              style: const TextStyle(fontSize: 12),
            ),
          ),
        const SizedBox(height: 12),
        TextField(
          controller: _frostMsgController,
          decoration: const InputDecoration(
            labelText: 'FROST message',
            border: OutlineInputBorder(),
            isDense: true,
          ),
        ),
        const SizedBox(height: 4),
        Row(
          children: [
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _frostKeygen,
                icon: const Icon(Icons.groups),
                label: const Text('Jury keys (2-of-3)'),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _frostAggregate,
                icon: const Icon(Icons.how_to_reg),
                label: const Text('Aggregate sig'),
              ),
            ),
          ],
        ),
        if (_frostResult != null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: SelectableText('FROST (experimental):\n$_frostResult',
                style: const TextStyle(fontSize: 12)),
          ),
        if (_frostSharesJson != null && _frostResult == null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: SelectableText('FROST (experimental): jury keys generated',
                style: const TextStyle(fontSize: 12)),
          ),
        const SizedBox(height: 12),
        Row(
          children: [
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _pirGenerate,
                icon: const Icon(Icons.search),
                label: const Text('PIR query'),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _pirEvaluate,
                icon: const Icon(Icons.functions),
                label: const Text('PIR evaluate'),
              ),
            ),
          ],
        ),
        if (_pirResult != null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: SelectableText('PIR (experimental): $_pirResult',
                style: const TextStyle(fontSize: 12)),
          ),
        if (_pirQueryJson != null && _pirResult == null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: SelectableText('PIR (experimental): query generated',
                style: const TextStyle(fontSize: 12)),
          ),
        const Divider(),
        Text('SQL console', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 4),
        TextField(
          controller: _sqlController,
          decoration: const InputDecoration(
            labelText: 'SELECT …',
            border: OutlineInputBorder(),
            isDense: true,
          ),
          onSubmitted: (_) => _sqlRun(),
        ),
        const SizedBox(height: 4),
        Row(
          children: [
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _sqlRun,
                icon: const Icon(Icons.table_view),
                label: const Text('Run query'),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _applyZkRollup,
                icon: const Icon(Icons.integration_instructions),
                label: const Text('Apply ZK rollup'),
              ),
            ),
          ],
        ),
        if (_sqlResult != null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: SelectableText(_sqlResult!,
                style: const TextStyle(fontSize: 12)),
          ),
        if (_zkResult != null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: SelectableText('ZK: $_zkResult',
                style: const TextStyle(fontSize: 12)),
          ),
        const Divider(),
        Text('Telemetry', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 4),
        Row(
          children: [
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _recordTelemetryEvent,
                icon: const Icon(Icons.add_alert),
                label: const Text('Record event'),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _dumpTelemetry,
                icon: const Icon(Icons.archive),
                label: const Text('Dump (seals)'),
              ),
            ),
          ],
        ),
        const SizedBox(height: 8),
        ListTile(
          leading: Icon(
            _telemetrySealed ? Icons.lock : Icons.lock_open,
            color: _telemetrySealed ? Colors.orange : Colors.green,
          ),
          title:
              Text(_telemetrySealed ? 'Sealed (post-crash)' : 'Recorder live'),
          subtitle: Text(_telemetryInfo ?? ''),
        ),
        const SizedBox(height: 8),
        if (_telemetryTranscript != null)
          Container(
            padding: const EdgeInsets.all(12),
            decoration: BoxDecoration(
              color: Theme.of(context).colorScheme.surfaceContainerHighest,
              borderRadius: BorderRadius.circular(8),
            ),
            child: SelectableText(
              _telemetryTranscript!,
              style: const TextStyle(fontSize: 12),
            ),
          ),
        const Divider(),
        Text('DB & Mesh maintenance',
            style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 4),
        Row(
          children: [
            Expanded(
              child: OutlinedButton.icon(
                onPressed: () => _dbPurge(7 * 24 * 3600),
                icon: const Icon(Icons.delete_outline),
                label: const Text('Delete posts >7d'),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _dbPurgeAll,
                icon: const Icon(Icons.delete_forever),
                label: const Text('Delete all posts'),
              ),
            ),
          ],
        ),
        const SizedBox(height: 8),
        OutlinedButton.icon(
          onPressed: _purgeGeohashPeers,
          icon: const Icon(Icons.location_off),
          label: const Text('Purge stale geohash peers (7d)'),
        ),
        const SizedBox(height: 8),
        Row(
          children: [
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _pruneMeshLinks,
                icon: const Icon(Icons.link_off),
                label: const Text('Prune mesh links'),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _pruneMeshRoutes,
                icon: const Icon(Icons.route),
                label: const Text('Prune mesh routes'),
              ),
            ),
          ],
        ),
        const SizedBox(height: 8),
        OutlinedButton.icon(
          onPressed: _meshNodeId,
          icon: const Icon(Icons.fingerprint),
          label: const Text('Skademlia node id (active account)'),
        ),
        const SizedBox(height: 8),
        TextField(
          controller: _nsecController,
          obscureText: true,
          decoration: const InputDecoration(
            labelText: 'nsec (signer pubkey derivation probe)',
            border: OutlineInputBorder(),
            isDense: true,
          ),
          onSubmitted: (_) => _signerFromNsec(),
        ),
        const SizedBox(height: 4),
        OutlinedButton.icon(
          onPressed: _signerFromNsec,
          icon: const Icon(Icons.key),
          label: const Text('Derive pubkey'),
        ),
        if (_meshResult != null) ...[
          const SizedBox(height: 4),
          SelectableText(_meshResult!, style: const TextStyle(fontSize: 12)),
        ],
        const Divider(),
        Text('Diagnostics', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 4),
        if (_diagError != null)
          Text('Unavailable: $_diagError')
        else if (_diagnostics != null)
          Container(
            padding: const EdgeInsets.all(12),
            decoration: BoxDecoration(
              color: Theme.of(context).colorScheme.surfaceContainerHighest,
              borderRadius: BorderRadius.circular(8),
            ),
            child: SelectableText(
              const JsonEncoder.withIndent('  ').convert(_diagnostics),
              style: const TextStyle(fontSize: 12),
            ),
          ),
        const SizedBox(height: 8),
        OutlinedButton.icon(
          onPressed: _load,
          icon: const Icon(Icons.refresh),
          label: const Text('Refresh'),
        ),
      ],
    );
  }
}
