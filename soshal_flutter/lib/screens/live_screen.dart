import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/friends_service.dart';
import '../services/session_service.dart';
import '../services/streaming_service.dart';
import '../services/p2p_service.dart';
import '../utils/format.dart';
import '../widgets/app_snack.dart';
import '../widgets/audience_filter_dropdown.dart';
import '../widgets/empty_state.dart';

/// Live streams: presence list + start/end own stream.
class LiveScreen extends StatefulWidget {
  /// Live screen.
  const LiveScreen({super.key});

  @override
  State<LiveScreen> createState() => _LiveScreenState();
}

class _LiveScreenState extends State<LiveScreen> {
  bool _loading = true;
  AudienceFilter _audienceFilter = AudienceFilter.all;

  @override
  void initState() {
    super.initState();
    final pubkey = context.read<SessionService>().activePubkey;
    if (pubkey != null) {
      context.friendsServiceReadOrNull?.loadAudienceGraph(pubkey);
    }
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final api = context.read<StreamingService>();
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey != null) {
        context.friendsServiceReadOrNull?.loadAudienceGraph(pubkey);
      }
      await api.fetchLive();
    } catch (e) {
      debugPrint('live load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _startLiveDialog() async {
    final title = TextEditingController();
    final desc = TextEditingController();
    final url =
        TextEditingController(text: 'https://stream.soshal.example/live');
    var canGo = false;

    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) {
          void update() {
            setDialogState(() {
              canGo = title.text.trim().isNotEmpty &&
                  title.text.trim().length <= 300 &&
                  _isValidLiveUrl(url.text);
            });
          }

          return AlertDialog(
            title: const Text('Go live'),
            content: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextField(
                  controller: title,
                  onChanged: (_) => update(),
                  decoration: const InputDecoration(labelText: 'Title *'),
                ),
                TextField(
                  controller: desc,
                  maxLines: 2,
                  decoration: const InputDecoration(labelText: 'Description'),
                ),
                TextField(
                  controller: url,
                  onChanged: (_) => update(),
                  decoration: const InputDecoration(labelText: 'Stream URL *'),
                ),
              ],
            ),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(context, false),
                child: const Text('Cancel'),
              ),
              FilledButton(
                onPressed: canGo ? () => Navigator.pop(context, true) : null,
                child: const Text('Go live'),
              ),
            ],
          );
        },
      ),
    );

    // MoQ transport: a live stream is a real QUIC-backed broadcast (group
    // registry on the LAN). The publisher session opens here.
    if (ok == true) {
      if (!mounted) return;
      try {
        final session = context.read<SessionService>();
        final pubkey = session.activePubkey;
        if (pubkey == null) throw Exception('Sign in to go live');
        final api = context.read<StreamingService>();
        final eventId = await api.startLive(
          pubkey,
          title.text.trim(),
          desc.text.trim(),
          url.text.trim(),
        );
        await api.startMoqBroadcast(
            streamId: eventId, title: title.text.trim());
        await _load();
        if (!mounted) return;
        await context.push(
          '/live/broadcast/$eventId?title=${Uri.encodeQueryComponent(title.text.trim())}',
        );
        await _load();
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context)
              .showSnackBar(SnackBar(content: SelectableText('Failed: $e')));
        }
      }
    }
  }

  /// A real ingest endpoint, not a placeholder: http(s) URL with a concrete
  /// host (blocks the `.example` placeholder that pre-filled the dialog and
  /// silently produced a dead "live" stream).
  bool _isValidLiveUrl(String s) {
    final t = s.trim();
    if (t.isEmpty || t.length > 500) return false;
    final uri = Uri.tryParse(t);
    if (uri == null || !uri.hasScheme || uri.host.isEmpty) return false;
    final scheme = uri.scheme.toLowerCase();
    if (scheme != 'http' && scheme != 'https') return false;
    if (uri.host == 'example.com' ||
        uri.host.endsWith('.example') ||
        uri.host.contains('soshal.example')) {
      return false;
    }
    return true;
  }

  Future<void> _endStream(StreamRow stream) async {
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) return;
      final api = context.read<StreamingService>();
      await api.stopMoqBroadcast();
      await api.endLive(stream.id, pubkey);
      await _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('End failed: $e')));
      }
    }
  }

  Future<void> _joinStream(StreamRow stream) async {
    final transport = context.read<P2pService>();
    P2pPeerDto? peer;
    for (final p in transport.peers.reversed) {
      if (p.quicPort != null) {
        peer = p;
        break;
      }
    }
    final peerIp = peer?.ip;
    final quicPort = peer?.quicPort;
    if (peerIp == null || quicPort == null) {
      _toast('No LAN peer advertising a QUIC stream port');
      return;
    }
    if (!mounted) return;
    await context.push(
      '/live/viewer/${stream.id}?addr=${Uri.encodeQueryComponent('$peerIp:$quicPort')}',
    );
  }

  void _toast(String msg) {
    if (!mounted) return;
    showAppSnack(context, msg);
  }

  @override
  Widget build(BuildContext context) {
    final me = context.select<SessionService, String?>((s) => s.activePubkey);

    return Scaffold(
      appBar: AppBar(
        title: const Text('Live'),
        actions: [
          AudienceFilterDropdown(
            value: _audienceFilter,
            onChanged: (filter) => setState(() => _audienceFilter = filter),
          ),
        ],
      ),
      floatingActionButton: FloatingActionButton(
        onPressed: _startLiveDialog,
        tooltip: 'Go live',
        child: const Icon(Icons.videocam),
      ),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : Column(
              children: [
                Expanded(
                  child: Consumer<StreamingService>(
                    builder: (context, api, _) {
                      final friendsService = context.friendsServiceOrNull;
                      final filtered = friendsService != null
                          ? friendsService.filterList(
                              api.live,
                              _audienceFilter,
                              (s) => s.pubkey,
                              myPubkey: me,
                            )
                          : api.live;
                      if (filtered.isEmpty) {
                        return EmptyState(
                          icon: Icons.videocam_off_outlined,
                          title: _audienceFilter == AudienceFilter.all
                              ? 'No streams right now'
                              : 'No streams match ${_audienceFilter.label}',
                        );
                      }
                      return RefreshIndicator(
                        onRefresh: _load,
                        child: ListView.builder(
                          itemExtent: 72.0,
                          itemCount: filtered.length,
                          itemBuilder: (context, index) {
                            final s = filtered[index];
                            final isMine = s.pubkey == me;
                            return ListTile(
                              leading: CircleAvatar(
                                backgroundColor: Colors.red.shade600,
                                child: const Icon(Icons.live_tv,
                                    color: Colors.white),
                              ),
                              title: Text(s.title),
                              subtitle: Text(
                                '${isMine ? 'You · ' : ''}${firstChars(s.pubkey, 12)}'
                                '${s.summary.isNotEmpty ? '\n${s.summary}' : ''}',
                                maxLines: 2,
                                overflow: TextOverflow.ellipsis,
                              ),
                              isThreeLine: true,
                              trailing: isMine
                                  ? TextButton(
                                      onPressed: () => _endStream(s),
                                      child: const Text('End'),
                                    )
                                  : TextButton(
                                      onPressed: () => _joinStream(s),
                                      child: const Text('Join'),
                                    ),
                            );
                          },
                        ),
                      );
                    },
                  ),
                ),
              ],
            ),
    );
  }
}
