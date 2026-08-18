import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/chatrandom_service.dart';
import '../services/session_service.dart';
import '../utils/format.dart';
import '../widgets/app_snack.dart';

/// Chat Random: interest-based random pairing with strangers via relay
/// availability announcements and request/accept events.
class ChatRandomScreen extends StatefulWidget {
  /// Chat Random screen.
  const ChatRandomScreen({super.key});

  @override
  State<ChatRandomScreen> createState() => _ChatRandomScreenState();
}

class _ChatRandomScreenState extends State<ChatRandomScreen> {
  final TextEditingController _interestsController = TextEditingController();
  String _mediaType = 'video';
  String _mode = 'public';
  String _availabilityJson = '';
  bool _searching = false;
  String? _pubkey;
  Timer? _pollTimer;
  int _pollCount = 0;

  static const _pollInterval = Duration(seconds: 5);
  static const _maxPolls = 6;

  @override
  void initState() {
    super.initState();
    _pubkey = context.read<SessionService>().activePubkey;
    if (_pubkey != null) {
      _refreshAvailability();
      _fetchOnce();
    }
  }

  @override
  void dispose() {
    _pollTimer?.cancel();
    _interestsController.dispose();
    super.dispose();
  }

  List<String> _interests() {
    return _interestsController.text
        .split(',')
        .map((s) => s.trim())
        .where((s) => s.isNotEmpty)
        .toList();
  }

  void _refreshAvailability() {
    try {
      final json = context.read<ChatrandomService>().availableContent(
            interests: _interests(),
            mediaType: _mediaType,
            mode: _mode,
          );
      if (mounted) setState(() => _availabilityJson = json);
    } catch (e) {
      debugPrint('chatrandom availability: $e');
    }
  }

  Future<void> _fetchOnce() async {
    final pubkey = _pubkey;
    if (pubkey == null) return;
    try {
      await context.read<ChatrandomService>().fetch(pubkey, limit: 20);
    } catch (e) {
      debugPrint('chatrandom fetch: $e');
    }
  }

  Future<void> _findPeer() async {
    final pubkey = _pubkey;
    if (pubkey == null) {
      _showSnack('Sign in required to find peers');
      return;
    }
    if (_searching) return;
    setState(() => _searching = true);
    try {
      final contentJson = context.read<ChatrandomService>().availableContent(
            interests: _interests(),
            mediaType: _mediaType,
            mode: _mode,
          );
      await context.read<ChatrandomService>().send(
            requestType: 'request',
            peers: const [],
            contentJson: contentJson,
          );
      if (!mounted) return;
      _showSnack('Request sent. Listening for matches...');
      _startPolling(pubkey);
    } catch (e) {
      debugPrint('chatrandom find: $e');
      if (mounted) {
        setState(() => _searching = false);
        _showSnack('Find failed: $e');
      }
    }
  }

  void _startPolling(String pubkey) {
    _pollTimer?.cancel();
    _pollCount = 0;
    _poll();
    _pollTimer = Timer.periodic(_pollInterval, (timer) {
      _pollCount++;
      if (_pollCount > _maxPolls) {
        timer.cancel();
        if (mounted) setState(() => _searching = false);
        return;
      }
      _poll();
    });
  }

  Future<void> _poll() async {
    final pubkey = _pubkey;
    if (pubkey == null) return;
    try {
      await context.read<ChatrandomService>().fetch(pubkey, limit: 20);
    } catch (e) {
      debugPrint('chatrandom poll: $e');
    }
  }

  Future<void> _accept(ChatrandomPeer peer) async {
    try {
      await context.read<ChatrandomService>().send(
            requestType: 'accept',
            peers: [peer.pubkey],
            contentJson: '',
          );
      if (mounted) _showSnack('Match accepted with ${_short(peer.pubkey)}');
    } catch (e) {
      if (mounted) _showSnack('Accept failed: $e');
    }
  }

  void _showSnack(String message) {
    showAppSnack(context, message);
  }

  String _short(String pubkey) => prefixEllipsis(pubkey, 16);

  final Map<String, Map<String, dynamic>> _peerContentCache = {};

  Map<String, dynamic> _peerContent(ChatrandomPeer peer) {
    return _peerContentCache.putIfAbsent(peer.content, () {
      try {
        final decoded = jsonDecode(peer.content);
        return decoded is Map<String, dynamic> ? decoded : const {};
      } catch (_) {
        return const {};
      }
    });
  }

  List<Widget> _headerChildren(ChatrandomService service) {
    return [
      Text('Your Status', style: Theme.of(context).textTheme.titleMedium),
      const SizedBox(height: 8),
      TextField(
        controller: _interestsController,
        decoration: const InputDecoration(
          labelText: 'Interests (comma-separated)',
          hintText: 'music, tech, gaming',
          border: OutlineInputBorder(),
        ),
        onChanged: (_) => _refreshAvailability(),
      ),
      const SizedBox(height: 8),
      if (_interests().isNotEmpty)
        Wrap(
          spacing: 6,
          runSpacing: 6,
          children: _interests()
              .map((i) => Chip(
                    label: Text(i),
                    visualDensity: VisualDensity.compact,
                  ))
              .toList(),
        ),
      const SizedBox(height: 12),
      Text('Media type', style: Theme.of(context).textTheme.labelLarge),
      const SizedBox(height: 4),
      SegmentedButton<String>(
        segments: const [
          ButtonSegment(value: 'voice', label: Text('Voice')),
          ButtonSegment(value: 'video', label: Text('Video')),
          ButtonSegment(value: 'text', label: Text('Text')),
        ],
        selected: {_mediaType},
        onSelectionChanged: (s) {
          setState(() => _mediaType = s.first);
          _refreshAvailability();
        },
      ),
      const SizedBox(height: 12),
      Text('Mode', style: Theme.of(context).textTheme.labelLarge),
      const SizedBox(height: 4),
      SegmentedButton<String>(
        segments: const [
          ButtonSegment(value: 'public', label: Text('Public')),
          ButtonSegment(value: 'private', label: Text('Private')),
        ],
        selected: {_mode},
        onSelectionChanged: (s) {
          setState(() => _mode = s.first);
          _refreshAvailability();
        },
      ),
      const SizedBox(height: 12),
      if (_availabilityJson.isNotEmpty)
        Text(
          'Availability: $_availabilityJson',
          style: Theme.of(context).textTheme.bodySmall?.copyWith(
                fontFamily: 'monospace',
                color: Theme.of(context).colorScheme.outline,
              ),
        ),
      const SizedBox(height: 12),
      if (_searching) const LinearProgressIndicator(),
      const SizedBox(height: 12),
      FilledButton.icon(
        onPressed: _searching ? null : _findPeer,
        icon: const Icon(Icons.people_outline),
        label: const Text('Find a peer'),
      ),
      const SizedBox(height: 24),
      Text('Peers (${service.peers.length})',
          style: Theme.of(context).textTheme.titleMedium),
      const SizedBox(height: 4),
      if (service.peers.isEmpty)
        const Padding(
          padding: EdgeInsets.symmetric(vertical: 8),
          child: Text('No peers yet. Find a peer above.'),
        ),
    ];
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Chat Random')),
      body: _pubkey == null
          ? const Center(child: Text('Sign in required'))
          : Consumer<ChatrandomService>(
              builder: (context, service, _) {
                final headers = _headerChildren(service);
                return RefreshIndicator(
                  onRefresh: _fetchOnce,
                  child: ListView.builder(
                    padding: const EdgeInsets.all(16),
                    itemCount: headers.length + service.peers.length,
                    itemBuilder: (context, index) {
                      if (index < headers.length) return headers[index];
                      final peer = service.peers[index - headers.length];
                      final content = _peerContent(peer);
                      final interests =
                          (content['interests'] as List?) ?? const [];
                      final mediaType =
                          (content['media_type'] as String?) ?? '';
                      final mode = (content['mode'] as String?) ?? '';
                      final detail = [
                        if (interests.isNotEmpty) interests.join(', '),
                        if (mediaType.isNotEmpty) mediaType,
                        if (mode.isNotEmpty) mode,
                      ].join(' · ');
                      return Card(
                        margin: const EdgeInsets.symmetric(vertical: 4),
                        child: ListTile(
                          title: Text(_short(peer.pubkey)),
                          subtitle: detail.isEmpty
                              ? null
                              : Text(detail,
                                  maxLines: 2, overflow: TextOverflow.ellipsis),
                          trailing: FilledButton.tonal(
                            onPressed: () => _accept(peer),
                            child: const Text('Accept'),
                          ),
                        ),
                      );
                    },
                  ),
                );
              },
            ),
    );
  }
}
