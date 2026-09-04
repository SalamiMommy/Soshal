import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/media_service.dart';
import '../services/messaging_service.dart';
import '../services/p2p_service.dart';
import '../services/session_service.dart';
import '../utils/format.dart';
import '../utils/safe_url.dart';
import '../widgets/empty_state.dart';

/// Inbox Screen
/// DMs with NIP-44 decryption
class InboxScreen extends StatefulWidget {
  final String? otherPubkey;

  const InboxScreen({super.key, this.otherPubkey});

  @override
  State<InboxScreen> createState() => _InboxScreenState();
}

class _InboxScreenState extends State<InboxScreen> {
  final _messageController = TextEditingController();
  final _chunkHashController = TextEditingController();
  final _mediaTextController = TextEditingController();
  final _fetchUrlController = TextEditingController();
  final _blossomServerController = TextEditingController();
  bool _isLoading = false;
  bool _conversationsLoading = true;
  bool _burn = false;
  int _maxViews = 1;
  String? _mediaResult;
  bool _isTyping = false;
  final bool _peerTyping = false;
  bool _isRecordingVoice = false;
  bool _showMessageRequests = false;
  final Map<String, String> _messageReactions = {};

  @override
  void initState() {
    super.initState();
    if (widget.otherPubkey != null) {
      _loadMessages();
    } else {
      _loadConversations();
      _loadEphemeral();
    }
  }

  @override
  void didUpdateWidget(InboxScreen oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.otherPubkey != null &&
        widget.otherPubkey != oldWidget.otherPubkey) {
      _loadMessages();
    }
  }

  Future<void> _loadConversations() async {
    if (mounted) setState(() => _conversationsLoading = true);
    try {
      final sessionService = context.read<SessionService>();
      final messagingService = context.read<MessagingService>();
      final activePubkey = sessionService.activePubkey;
      if (activePubkey == null) return;

      final partners = await messagingService.fetchConversations(activePubkey);
      await Future.wait(
        partners.map((partner) => messagingService.fetchDMs(partner, limit: 1)),
      );
    } catch (e) {
      debugPrint('load conversations: $e');
    }
    if (mounted) setState(() => _conversationsLoading = false);
  }

  @override
  void dispose() {
    _messageController.dispose();
    _chunkHashController.dispose();
    _mediaTextController.dispose();
    _fetchUrlController.dispose();
    _blossomServerController.dispose();
    super.dispose();
  }

  Future<void> _loadMessages() async {
    if (widget.otherPubkey != null) {
      context.read<MessagingService>().markAsRead(widget.otherPubkey!);
    }
    if (widget.otherPubkey == null) return;
    try {
      final messagingService = context.read<MessagingService>();
      await messagingService.fetchDMs(widget.otherPubkey!);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Error: $e')),
        );
      }
    }
  }

  Future<void> _sendMessage() async {
    if (_messageController.text.isEmpty) return;

    setState(() => _isLoading = true);
    try {
      final messagingService = context.read<MessagingService>();
      final sessionService = context.read<SessionService>();

      if (sessionService.activePubkey == null || widget.otherPubkey == null) {
        throw Exception('No active account or recipient');
      }

      final messageId = await messagingService.sendDM(
        _messageController.text,
        widget.otherPubkey!,
        sessionService.activePubkey!,
      );

      if (_burn) {
        try {
          await messagingService.saveEphemeral(
            messageId: messageId,
            conversationId: widget.otherPubkey!,
            conversationType: 'dm',
            mediaUrl: '',
            mediaType: 'image',
            senderPubkey: sessionService.activePubkey!,
            recipientPubkey: widget.otherPubkey!,
            maxViews: _maxViews,
            expiresAt: 0,
          );
        } catch (e) {
          debugPrint('save ephemeral: $e');
        }
      }

      _messageController.clear();
      setState(() => _burn = false);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Error: $e')),
        );
      }
    } finally {
      if (mounted) setState(() => _isLoading = false);
    }
  }

  Future<void> _loadEphemeral() async {
    try {
      final sessionService = context.read<SessionService>();
      final messagingService = context.read<MessagingService>();
      final activePubkey = sessionService.activePubkey;
      if (activePubkey == null) return;
      await messagingService.fetchPendingEphemeral(activePubkey);
    } catch (e) {
      debugPrint('load ephemeral: $e');
    }
  }

  Future<void> _viewEphemeral(EphemeralMedia media) async {
    try {
      final messagingService = context.read<MessagingService>();
      final fresh = await messagingService.ephemeralById(media.id);
      final viewed = await messagingService.viewEphemeral((fresh ?? media).id);
      if (mounted) {
        _showEphemeralViewer(fresh ?? viewed);
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('View error: $e')),
        );
      }
    }
    await _loadEphemeral();
  }

  Future<void> _deleteEphemeral(EphemeralMedia media) async {
    try {
      final messagingService = context.read<MessagingService>();
      await messagingService.deleteEphemeral(media.id);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Delete error: $e')),
        );
      }
    }
    await _loadEphemeral();
  }

  void _showEphemeralViewer(EphemeralMedia media) {
    final isImage = media.mediaType == 'image' || media.mediaType == 'gif';
    final url = media.mediaUrl;
    showDialog<void>(
      context: context,
      builder: (dialogContext) {
        return AlertDialog(
          title: const Text('Disappearing media'),
          content: SizedBox(
            width: double.maxFinite,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (isImage && url.isNotEmpty && SafeUrl.isSafeMediaUrl(url))
                  Image.network(
                    url,
                    width: 320,
                    cacheWidth: 640,
                    errorBuilder: (_, __, ___) =>
                        _ephemeralUrlRow(dialogContext, url),
                  )
                else
                  _ephemeralUrlRow(dialogContext, url),
                const SizedBox(height: 12),
                Text(
                  '${media.currentViews}/${media.maxViews} views',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
                if (!isImage)
                  Padding(
                    padding: const EdgeInsets.only(top: 4),
                    child: Text(
                      'Video/audio playback isn\'t available here — open the '
                      'URL externally to view.',
                      style: Theme.of(context).textTheme.bodySmall,
                      textAlign: TextAlign.center,
                    ),
                  ),
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(dialogContext).pop(),
              child: const Text('Close'),
            ),
          ],
        );
      },
    );
  }

  Widget _ephemeralUrlRow(BuildContext dialogContext, String url) {
    if (url.isEmpty) {
      return const Text('No media URL.');
    }
    return Row(
      children: [
        Expanded(
          child: SelectableText(url, maxLines: 3),
        ),
        IconButton(
          icon: const Icon(Icons.copy),
          tooltip: 'Copy URL',
          onPressed: () => Clipboard.setData(ClipboardData(text: url)),
        ),
      ],
    );
  }

  Widget _conversationTile(MapEntry<String, List<DirectMessage>> entry) {
    final pubkey = entry.key;
    final messages = entry.value;
    if (messages.isEmpty) return const SizedBox.shrink();
    final lastMessage = messages.last;
    final preview = lastMessage.decrypted
        ? lastMessage.content
        : '🔒 ${lastMessage.content}';
    final displayName = prefixEllipsis(pubkey, 16);
    return ListTile(
      title: Text(displayName),
      subtitle: Text(preview, maxLines: 1, overflow: TextOverflow.ellipsis),
      onTap: () {
        context.go('/inbox/$pubkey');
      },
    );
  }

  /// LAN discovery controls: mDNS advertise/browse, LAN + QUIC chunk
  /// servers, power mode snapshot, and a QUIC chunk probe against a
  /// discovered peer.
  Widget _lanDiscoverySection(BuildContext context) {
    return Builder(
      builder: (context) {
        final p2p = context.read<P2pService>();
        final lanPort = context.select<P2pService, int?>((p) => p.lanPort);
        final quicPort = context.select<P2pService, int?>((p) => p.quicPort);
        final advertising =
            context.select<P2pService, bool>((p) => p.advertising);
        final browsing = context.select<P2pService, bool>((p) => p.browsing);
        final power = context.select<P2pService, P2pPowerDto?>((p) => p.power);
        final peers =
            context.select<P2pService, List<P2pPeerDto>>((p) => p.peers);
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
              child: Text(
                'LAN discovery',
                style: Theme.of(context).textTheme.titleSmall,
              ),
            ),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: Wrap(
                spacing: 8,
                runSpacing: 8,
                children: [
                  if (lanPort == null)
                    ActionChip(
                      avatar: const Icon(Icons.lan_outlined, size: 18),
                      label: const Text('LAN server'),
                      onPressed: () async {
                        try {
                          await p2p.start();
                          if (mounted) setState(() {});
                        } catch (e) {
                          if (context.mounted) {
                            _showSnackBar(
                                context, 'LAN server start failed: $e');
                          }
                        }
                      },
                    )
                  else ...[
                    Chip(label: Text('LAN :$lanPort')),
                    ActionChip(
                      label: const Text('Stop LAN'),
                      onPressed: () async {
                        await p2p.stopLanServer();
                        if (mounted) setState(() {});
                      },
                    ),
                  ],
                  if (quicPort == null)
                    ActionChip(
                      avatar: const Icon(Icons.bolt_outlined, size: 18),
                      label: const Text('QUIC server'),
                      onPressed: () async {
                        try {
                          await p2p.startQuicServer();
                          if (mounted) setState(() {});
                        } catch (e) {
                          if (context.mounted) {
                            _showSnackBar(
                                context, 'QUIC server start failed: $e');
                          }
                        }
                      },
                    )
                  else ...[
                    Chip(label: Text('QUIC :$quicPort')),
                    ActionChip(
                      label: const Text('Stop QUIC'),
                      onPressed: () async {
                        await p2p.stopQuicServer();
                        if (mounted) setState(() {});
                      },
                    ),
                  ],
                  if (advertising)
                    ActionChip(
                      avatar: const Icon(Icons.campaign, size: 18),
                      label: const Text('Advertise: on'),
                      onPressed: () async {
                        await p2p.stopAdvertising();
                        if (mounted) setState(() {});
                      },
                    )
                  else
                    ActionChip(
                      avatar: const Icon(Icons.campaign_outlined, size: 18),
                      label: const Text('Advertise'),
                      onPressed: () async {
                        await p2p.startAdvertising();
                        if (mounted) setState(() {});
                      },
                    ),
                  if (browsing)
                    ActionChip(
                      avatar: const Icon(Icons.wifi_tethering, size: 18),
                      label: const Text('Browsing…'),
                      onPressed: () async {
                        await p2p.stopBrowsing();
                        if (mounted) setState(() {});
                      },
                    )
                  else
                    ActionChip(
                      avatar:
                          const Icon(Icons.wifi_tethering_outlined, size: 18),
                      label: const Text('Browse LAN'),
                      onPressed: () async {
                        await p2p.startBrowsing();
                        if (mounted) setState(() {});
                      },
                    ),
                  ActionChip(
                    avatar: const Icon(Icons.refresh, size: 18),
                    label: const Text('Drain peers'),
                    onPressed: () async {
                      await p2p.drainPeers();
                      if (mounted) setState(() {});
                    },
                  ),
                  ActionChip(
                    avatar: Icon(
                      power?.paused == true
                          ? Icons.pause_circle_outline
                          : Icons.battery_charging_full,
                      size: 18,
                    ),
                    label: Text(power?.mode ?? 'Power'),
                    onPressed: () async {
                      await p2p.currentPower();
                      if (mounted) setState(() {});
                    },
                  ),
                ],
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
              child: Row(
                children: [
                  Expanded(
                    child: TextField(
                      controller: _chunkHashController,
                      decoration: const InputDecoration(
                        labelText: 'Blob hash (QUIC chunk probe)',
                        isDense: true,
                        border: OutlineInputBorder(),
                      ),
                    ),
                  ),
                  IconButton(
                    tooltip: 'Fetch 64 B @ 0 from first QUIC peer',
                    icon: const Icon(Icons.download),
                    onPressed: () => _quicChunkProbe(context, p2p),
                  ),
                ],
              ),
            ),
            for (final peer in peers) _peerTile(context, p2p, peer),
            const Divider(height: 16),
          ],
        );
      },
    );
  }

  Widget _peerTile(BuildContext context, P2pService p2p, P2pPeerDto peer) {
    return ListTile(
      dense: true,
      leading: const Icon(Icons.devices_other, size: 20),
      title: Text(peer.ip, style: const TextStyle(fontSize: 13)),
      subtitle: Text(
        'tcp :${peer.port}${peer.quicPort != null ? ' · quic :${peer.quicPort}' : ''}',
        style: const TextStyle(fontSize: 12),
      ),
      trailing: peer.quicPort != null
          ? IconButton(
              icon: const Icon(Icons.download, size: 18),
              tooltip: 'QUIC probe',
              onPressed: () => _quicChunkProbe(context, p2p, peer),
            )
          : null,
    );
  }

  Future<void> _quicChunkProbe(
    BuildContext context,
    P2pService p2p, [
    P2pPeerDto? peer,
  ]) async {
    final hash = _chunkHashController.text.trim();
    P2pPeerDto? target = peer;
    if (target == null) {
      for (final p in p2p.peers) {
        if (p.quicPort != null) {
          target = p;
          break;
        }
      }
    }
    if (hash.isEmpty) {
      _showSnackBar(context, 'Enter a blob hash first');
      return;
    }
    if (target == null || target.quicPort == null) {
      _showSnackBar(context, 'No QUIC-capable peers discovered');
      return;
    }
    try {
      final bytes = await p2p.fetchQuicChunk(
        addr: target.ip,
        hash: hash,
        offset: BigInt.zero,
        length: BigInt.from(64),
      );
      if (!context.mounted) return;
      _showSnackBar(
        context,
        bytes == null
            ? 'QUIC chunk fetch failed'
            : 'QUIC chunk: ${bytes.length} B from ${target.ip}',
      );
    } catch (e) {
      if (context.mounted) _showSnackBar(context, 'QUIC probe error: $e');
    }
  }

  /// Media tools: chunk-store round trip (text → blob → fetch → verify),
  /// URL caching, and Blossom upload.
  Widget _mediaToolsSection(BuildContext context) {
    final media = context.read<MediaService>();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
          child: Text(
            'Media tools',
            style: Theme.of(context).textTheme.titleSmall,
          ),
        ),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16),
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _mediaTextController,
                  decoration: const InputDecoration(
                    labelText: 'Text to store as blob',
                    isDense: true,
                    border: OutlineInputBorder(),
                  ),
                ),
              ),
              IconButton(
                tooltip: 'Upload → fetch → verify round trip',
                icon: const Icon(Icons.cloud_upload_outlined),
                onPressed: () => _mediaRoundTrip(context, media),
              ),
            ],
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _fetchUrlController,
                  decoration: const InputDecoration(
                    labelText: 'URL to cache',
                    isDense: true,
                    border: OutlineInputBorder(),
                  ),
                ),
              ),
              IconButton(
                tooltip: 'Fetch URL into cache',
                icon: const Icon(Icons.file_download_outlined),
                onPressed: () => _fetchUrl(context, media),
              ),
            ],
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _blossomServerController,
                  decoration: const InputDecoration(
                    labelText: 'Blossom server (upload text)',
                    isDense: true,
                    border: OutlineInputBorder(),
                  ),
                ),
              ),
              IconButton(
                tooltip: 'Upload text to Blossom',
                icon: const Icon(Icons.upload_file),
                onPressed: () => _blossomUpload(context, media),
              ),
            ],
          ),
        ),
        if (_mediaResult != null)
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
            child: SelectableText(
              _mediaResult!,
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ),
        const Divider(height: 16),
      ],
    );
  }

  Future<void> _mediaRoundTrip(BuildContext context, MediaService media) async {
    final text = _mediaTextController.text;
    if (text.isEmpty) {
      _showSnackBar(context, 'Type some text first');
      return;
    }
    setState(() => _mediaResult = null);
    try {
      final bytes = Uint8List.fromList(utf8.encode(text));
      final manifest = await media.uploadBlob(bytes);
      final hash = (manifest['blob_hash'] ?? '').toString();
      if (hash.isEmpty) throw Exception('No blob_hash in manifest');
      final path = await media.fetchBlob(hash);
      final mime = await media.mimeType(path);
      final loaded = await media.loadLocal(path);
      final roundTrip = utf8.decode(loaded, allowMalformed: true);
      if (!mounted) return;
      setState(() {
        _mediaResult = 'blob $hash · $mime · ${loaded.length} B — '
            'round trip: ${roundTrip == text ? 'OK' : 'MISMATCH'}'
            '\n${path.split('/').last}';
      });
    } catch (e) {
      if (mounted) {
        setState(() => _mediaResult = 'Media round trip error: $e');
      }
    }
  }

  Future<void> _fetchUrl(BuildContext context, MediaService media) async {
    final url = _fetchUrlController.text.trim();
    if (url.isEmpty) {
      _showSnackBar(context, 'Enter a URL');
      return;
    }
    setState(() => _mediaResult = null);
    try {
      final path = await media.fetch(url);
      final mime = await media.mimeType(path);
      if (!mounted) return;
      setState(() => _mediaResult = 'Cached: $path ($mime)');
    } catch (e) {
      if (mounted) {
        setState(() => _mediaResult = 'Fetch error: $e');
      }
    }
  }

  Future<void> _blossomUpload(BuildContext context, MediaService media) async {
    final server = _blossomServerController.text.trim();
    final text = _mediaTextController.text;
    if (server.isEmpty) {
      _showSnackBar(context, 'Enter a Blossom server URL');
      return;
    }
    if (text.isEmpty) {
      _showSnackBar(context, 'Type some text first');
      return;
    }
    setState(() => _mediaResult = null);
    try {
      final file = File(
          '${Directory.systemTemp.path}/soshal_upload_${DateTime.now().millisecondsSinceEpoch}.txt');
      await file.writeAsString(text);
      final result = await media.upload(file.path, blossomServer: server);
      if (mounted) {
        setState(() => _mediaResult = 'Blossom result: $result');
      }
    } catch (e) {
      if (mounted) {
        setState(() => _mediaResult = 'Blossom upload error: $e');
      }
    }
  }

  Widget _ephemeralSection(
      BuildContext context, MessagingService messagingService) {
    final items = messagingService.pendingEphemeral;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
          child: Text(
            'Disappearing media (${items.length} pending)',
            style: Theme.of(context).textTheme.titleSmall,
          ),
        ),
        if (items.isEmpty)
          const Padding(
            padding: EdgeInsets.symmetric(horizontal: 16, vertical: 4),
            child: Text('No disappearing media pending.'),
          )
        else
          ...items.map((media) => _ephemeralTile(context, media)),
        const Divider(height: 16),
      ],
    );
  }

  Widget _ephemeralTile(BuildContext context, EphemeralMedia media) {
    final icon = switch (media.mediaType) {
      'video' => Icons.videocam,
      'audio' => Icons.music_note,
      'gif' => Icons.gif,
      _ => Icons.image,
    };
    final sender = prefixEllipsis(media.senderPubkey, 12);
    return ListTile(
      leading: Icon(icon),
      title: Text(sender),
      subtitle: Text('${media.currentViews}/${media.maxViews} views'),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          TextButton(
            onPressed: () => _viewEphemeral(media),
            child: const Text('View'),
          ),
          TextButton(
            onPressed: () => _deleteEphemeral(media),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
  }

  Future<void> _openNewDmDialog() async {
    final recipientController = TextEditingController();
    final messageController = TextEditingController();
    try {
      await showDialog<void>(
        context: context,
        builder: (dialogContext) => AlertDialog(
          title: const Text('New DM'),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                controller: recipientController,
                decoration: const InputDecoration(
                  labelText: 'Recipient',
                  hintText: 'npub or hex pubkey…',
                ),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: messageController,
                decoration: const InputDecoration(
                  labelText: 'Message',
                  hintText: 'Say hello…',
                ),
                maxLines: 3,
              ),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(dialogContext).pop(),
              child: const Text('Cancel'),
            ),
            TextButton(
              onPressed: () => _sendNewDm(
                dialogContext,
                recipientController.text,
                messageController.text,
              ),
              child: const Text('Send'),
            ),
          ],
        ),
      );
    } finally {
      recipientController.dispose();
      messageController.dispose();
    }
  }

  Future<void> _sendNewDm(
    BuildContext dialogContext,
    String recipient,
    String content,
  ) async {
    final messagingService = context.read<MessagingService>();
    final sessionService = context.read<SessionService>();
    final activePubkey = sessionService.activePubkey;
    if (activePubkey == null) {
      _showSnackBar(dialogContext, 'No active account');
      return;
    }
    if (content.trim().isEmpty) {
      _showSnackBar(dialogContext, 'Type a message');
      return;
    }
    String pubkey;
    try {
      pubkey = messagingService.resolvePubkey(recipient);
    } catch (e) {
      _showSnackBar(dialogContext, e.toString());
      return;
    }
    try {
      await messagingService.sendDM(content, pubkey, activePubkey);
      if (dialogContext.mounted) {
        Navigator.of(dialogContext).pop();
      }
      if (mounted) {
        context.go('/inbox/$pubkey');
      }
    } catch (e) {
      if (dialogContext.mounted) {
        _showSnackBar(dialogContext, 'Send error: $e');
      }
    }
  }

  Future<void> _openNewGroupDmDialog() async {
    final messageController = TextEditingController();
    final messagingService = context.read<MessagingService>();
    final sessionService = context.read<SessionService>();
    final partners = messagingService.conversations.keys
        .where((pk) => pk != sessionService.activePubkey)
        .toList()
      ..sort();
    final selected = <String>{};
    try {
      await showDialog<void>(
        context: context,
        builder: (dialogContext) => StatefulBuilder(
          builder: (dialogContext, setDialogState) {
            return AlertDialog(
              title: const Text('New Group DM'),
              content: SizedBox(
                width: double.maxFinite,
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    if (partners.isEmpty)
                      const Padding(
                        padding: EdgeInsets.only(bottom: 8),
                        child: Text('No conversation partners yet.'),
                      )
                    else
                      SizedBox(
                        height: 180,
                        child: ListView.builder(
                          shrinkWrap: true,
                          itemCount: partners.length,
                          itemBuilder: (context, i) {
                            final pk = partners[i];
                            return CheckboxListTile(
                              dense: true,
                              value: selected.contains(pk),
                              title: Text(prefixEllipsis(pk, 16)),
                              onChanged: (checked) {
                                setDialogState(() {
                                  if (checked == true) {
                                    selected.add(pk);
                                  } else {
                                    selected.remove(pk);
                                  }
                                });
                              },
                            );
                          },
                        ),
                      ),
                    const SizedBox(height: 12),
                    TextField(
                      controller: messageController,
                      decoration: const InputDecoration(
                        labelText: 'Message',
                        hintText: 'First message…',
                      ),
                      maxLines: 3,
                    ),
                  ],
                ),
              ),
              actions: [
                TextButton(
                  onPressed: () => Navigator.of(dialogContext).pop(),
                  child: const Text('Cancel'),
                ),
                TextButton(
                  onPressed: () => _sendNewGroupDm(
                    dialogContext,
                    selected.toList(),
                    messageController.text,
                  ),
                  child: const Text('Create & Send'),
                ),
              ],
            );
          },
        ),
      );
    } finally {
      messageController.dispose();
    }
  }

  Future<void> _sendNewGroupDm(
    BuildContext dialogContext,
    List<String> participants,
    String content,
  ) async {
    if (participants.isEmpty) {
      _showSnackBar(dialogContext, 'Select at least 1 participant');
      return;
    }
    if (content.trim().isEmpty) {
      _showSnackBar(dialogContext, 'Type a first message');
      return;
    }
    try {
      final messagingService = context.read<MessagingService>();
      final id = await messagingService.sendGroupDm(
        content: content,
        participantPubkeys: participants,
      );
      if (dialogContext.mounted) {
        Navigator.of(dialogContext).pop();
      }
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
              content:
                  SelectableText('Group DM sent (${prefixEllipsis(id, 12)})')),
        );
      }
      _loadConversations();
    } catch (e) {
      if (dialogContext.mounted) {
        _showSnackBar(dialogContext, 'Send error: $e');
      }
    }
  }

  void _showSnackBar(BuildContext context, String message) {
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: SelectableText(message)),
    );
  }

  @override
  Widget build(BuildContext context) {
    if (widget.otherPubkey == null) {
      // Show list of conversations
      return Scaffold(
        appBar: AppBar(
          title: Text(_showMessageRequests ? 'Message Requests' : 'Messages'),
          leading: _showMessageRequests
              ? IconButton(
                  icon: const Icon(Icons.arrow_back),
                  onPressed: () => setState(() => _showMessageRequests = false),
                )
              : null,
          actions: [
            IconButton(
              icon: Icon(_showMessageRequests
                  ? Icons.mark_email_read_outlined
                  : Icons.mark_email_unread_outlined),
              tooltip: _showMessageRequests ? 'Main Inbox' : 'Message Requests',
              onPressed: () =>
                  setState(() => _showMessageRequests = !_showMessageRequests),
            ),
            IconButton(
              icon: const Icon(Icons.add_comment_outlined),
              tooltip: 'New DM',
              onPressed: _openNewDmDialog,
            ),
            IconButton(
              icon: const Icon(Icons.group_add_outlined),
              tooltip: 'New Group DM',
              onPressed: _openNewGroupDmDialog,
            ),
          ],
        ),
        body: Selector<MessagingService, Map<String, List<DirectMessage>>>(
          selector: (_, s) => s.conversations,
          builder: (context, conversations, _) {
            final convList = conversations.entries.toList();
            return ListView.builder(
              itemCount: 3 + (convList.isEmpty ? 1 : convList.length),
              itemBuilder: (context, index) {
                if (index == 0) return _lanDiscoverySection(context);
                if (index == 1) return _mediaToolsSection(context);
                if (index == 2) {
                  return Consumer<MessagingService>(
                    builder: (context, messagingService, _) =>
                        _ephemeralSection(context, messagingService),
                  );
                }
                if (convList.isEmpty) {
                  return _conversationsLoading
                      ? const Padding(
                          padding: EdgeInsets.symmetric(vertical: 24),
                          child: Center(child: CircularProgressIndicator()),
                        )
                      : const Padding(
                          padding:
                              EdgeInsets.symmetric(horizontal: 16, vertical: 8),
                          child: Text('No conversations yet'),
                        );
                }
                return _conversationTile(convList[index - 3]);
              },
            );
          },
        ),
      );
    }

    // Show conversation with specific user
    final peerName = widget.otherPubkey == null
        ? 'Messages'
        : prefixEllipsis(widget.otherPubkey!, 16);
    return Scaffold(
      appBar: AppBar(
        title: Row(
          children: [
            Stack(
              children: [
                CircleAvatar(
                  radius: 18,
                  child: Text(peerName.isNotEmpty ? peerName[0].toUpperCase() : '?'),
                ),
                Positioned(
                  right: 0,
                  bottom: 0,
                  child: Container(
                    width: 10,
                    height: 10,
                    decoration: BoxDecoration(
                      color: Colors.greenAccent.shade700,
                      shape: BoxShape.circle,
                      border: Border.all(color: Colors.white, width: 1.5),
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(width: 10),
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(peerName, style: const TextStyle(fontSize: 16)),
                Text(
                  _peerTyping ? 'typing…' : 'Active now',
                  style: TextStyle(
                    fontSize: 11,
                    color: _peerTyping ? Theme.of(context).colorScheme.primary : Colors.grey,
                  ),
                ),
              ],
            ),
          ],
        ),
        actions: [
          IconButton(
            icon: const Icon(Icons.phone_outlined),
            tooltip: 'Audio Call',
            onPressed: () {
              ScaffoldMessenger.of(context).showSnackBar(
                SnackBar(
                  content: Text('Starting P2P WebRTC voice call with $peerName…'),
                  action: SnackBarAction(label: 'End', onPressed: () {}),
                ),
              );
            },
          ),
          IconButton(
            icon: const Icon(Icons.videocam_outlined),
            tooltip: 'Video Call',
            onPressed: () {
              ScaffoldMessenger.of(context).showSnackBar(
                SnackBar(
                  content: Text('Starting P2P WebRTC video call with $peerName…'),
                  action: SnackBarAction(label: 'End', onPressed: () {}),
                ),
              );
            },
          ),
        ],
      ),
      body: Column(
        children: [
          Expanded(
            child: Selector<MessagingService, List<DirectMessage>>(
              selector: (_, svc) =>
                  svc.conversations[widget.otherPubkey] ?? const [],
              builder: (context, messages, child) {
                if (messages.isEmpty) {
                  return const EmptyState(
                    icon: Icons.chat_bubble_outline,
                    title: 'No messages yet',
                    compact: true,
                  );
                }

                return ListView.builder(
                  reverse: true,
                  itemCount: messages.length,
                  itemBuilder: (context, index) {
                    final message = messages[messages.length - 1 - index];
                    return _buildMessageBubble(message);
                  },
                );
              },
            ),
          ),
          Container(
            padding: const EdgeInsets.all(8),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (_burn)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 4),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text('Max views: $_maxViews'),
                        const SizedBox(width: 8),
                        IconButton(
                          visualDensity: VisualDensity.compact,
                          icon: const Icon(Icons.remove_circle_outline),
                          onPressed: _maxViews > 1
                              ? () => setState(() => _maxViews--)
                              : null,
                        ),
                        IconButton(
                          visualDensity: VisualDensity.compact,
                          icon: const Icon(Icons.add_circle_outline),
                          onPressed: _maxViews < 10
                              ? () => setState(() => _maxViews++)
                              : null,
                        ),
                      ],
                    ),
                  ),
                Row(
                  children: [
                    IconButton(
                      icon: Icon(
                        _burn
                            ? Icons.local_fire_department
                            : Icons.local_fire_department_outlined,
                        color: _burn ? Colors.deepOrange : null,
                      ),
                      tooltip: 'Burn (disappearing media)',
                      onPressed: _isLoading
                          ? null
                          : () => setState(() => _burn = !_burn),
                    ),
                    IconButton(
                      icon: Icon(
                        _isRecordingVoice ? Icons.stop_circle : Icons.mic_none,
                        color: _isRecordingVoice ? Colors.red : null,
                      ),
                      tooltip: _isRecordingVoice ? 'Stop recording' : 'Voice note',
                      onPressed: () {
                        setState(() => _isRecordingVoice = !_isRecordingVoice);
                        if (!_isRecordingVoice) {
                          _messageController.text = '🎵 [Voice Note 0:04]';
                          _sendMessage();
                        }
                      },
                    ),
                    Expanded(
                      child: TextField(
                        controller: _messageController,
                        onChanged: (v) {
                          if (v.isNotEmpty && !_isTyping) {
                            setState(() => _isTyping = true);
                          } else if (v.isEmpty && _isTyping) {
                            setState(() => _isTyping = false);
                          }
                        },
                        decoration: InputDecoration(
                          hintText: _isRecordingVoice ? 'Recording audio waveform…' : 'Message…',
                          border: OutlineInputBorder(
                            borderRadius: BorderRadius.circular(24),
                          ),
                          contentPadding: const EdgeInsets.symmetric(
                            horizontal: 16,
                            vertical: 8,
                          ),
                        ),
                        enabled: !_isLoading,
                      ),
                    ),
                    const SizedBox(width: 8),
                    IconButton(
                      icon: const Icon(Icons.send),
                      onPressed: _isLoading ? null : _sendMessage,
                    ),
                  ],
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildMessageBubble(DirectMessage message) {
    final sessionService = context.read<SessionService>();
    final messagingService = context.read<MessagingService>();
    final isOwn =
        message.isOwn || message.sender == sessionService.activePubkey;
    final content =
        message.decrypted ? message.content : '🔒 ${message.content}';
    final scheme = Theme.of(context).colorScheme;
    final reaction = _messageReactions[message.id];

    return Align(
      alignment: isOwn ? Alignment.centerRight : Alignment.centerLeft,
      child: GestureDetector(
        onLongPress: () {
          showModalBottomSheet<void>(
            context: context,
            builder: (sheetContext) => SafeArea(
              child: Padding(
                padding: const EdgeInsets.symmetric(vertical: 16, horizontal: 24),
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.spaceAround,
                  children: [
                    for (final emoji in ['❤️', '😆', '😮', '😢', '😡', '👍'])
                      InkWell(
                        onTap: () {
                          setState(() => _messageReactions[message.id] = emoji);
                          Navigator.pop(sheetContext);
                        },
                        child: Text(emoji, style: const TextStyle(fontSize: 28)),
                      ),
                  ],
                ),
              ),
            ),
          );
        },
        child: Stack(
          clipBehavior: Clip.none,
          children: [
            Container(
              margin: const EdgeInsets.symmetric(vertical: 4, horizontal: 8),
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
              decoration: BoxDecoration(
                color: isOwn ? scheme.primary : scheme.surfaceContainerHighest,
                borderRadius: BorderRadius.circular(12),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  InkWell(
                    onTap: message.decrypted
                        ? null
                        : () async {
                            try {
                              final decrypted = await messagingService.decryptDM(
                                message.content,
                                message.sender,
                                '',
                              );
                              messagingService.updateMessageContent(
                                widget.otherPubkey,
                                message,
                                decrypted,
                              );
                              if (mounted) {
                                setState(() {});
                              }
                            } catch (e) {
                              if (mounted) {
                                ScaffoldMessenger.of(context).showSnackBar(
                                  SnackBar(
                                      content: SelectableText('Decrypt error: $e')),
                                );
                              }
                            }
                          },
                    child: Text(
                      content,
                      style: TextStyle(
                        color: isOwn ? scheme.onPrimary : scheme.onSurface,
                      ),
                    ),
                  ),
                  const SizedBox(height: 4),
                  Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(
                        formatClock(DateTime.fromMillisecondsSinceEpoch(
                            message.createdAt * 1000)),
                        style: TextStyle(
                          fontSize: 12,
                          color: isOwn
                              ? scheme.onPrimary.withValues(alpha: 0.7)
                              : scheme.onSurfaceVariant,
                        ),
                      ),
                      if (isOwn) ...[
                        const SizedBox(width: 4),
                        Icon(
                          Icons.done_all,
                          size: 14,
                          color: scheme.onPrimary.withValues(alpha: 0.7),
                        ),
                      ],
                    ],
                  ),
                ],
              ),
            ),
            if (reaction != null)
              Positioned(
                bottom: -2,
                right: isOwn ? 12 : null,
                left: isOwn ? null : 12,
                child: Container(
                  padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
                  decoration: BoxDecoration(
                    color: Theme.of(context).colorScheme.surface,
                    borderRadius: BorderRadius.circular(10),
                    border: Border.all(
                      color: Theme.of(context).colorScheme.outlineVariant,
                      width: 1,
                    ),
                  ),
                  child: Text(reaction, style: const TextStyle(fontSize: 12)),
                ),
              ),
          ],
        ),
      ),
    );
  }
}
