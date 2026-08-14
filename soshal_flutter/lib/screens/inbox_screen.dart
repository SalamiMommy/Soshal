import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/messaging_service.dart';
import '../services/session_service.dart';

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
  bool _isLoading = false;
  bool _burn = false;
  int _maxViews = 1;

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
    try {
      final sessionService = context.read<SessionService>();
      final messagingService = context.read<MessagingService>();
      final activePubkey = sessionService.activePubkey;
      if (activePubkey == null) return;

      final partners = await messagingService.fetchConversations(activePubkey);
      for (final partner in partners) {
        await messagingService.fetchDMs(partner);
      }
    } catch (e) {
      debugPrint('load conversations: $e');
    }
  }

  @override
  void dispose() {
    _messageController.dispose();
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
        '', // In real implementation, would get this from secure storage
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
      setState(() => _isLoading = false);
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
      final viewed = await messagingService.viewEphemeral(media.id);
      if (mounted) {
        _showEphemeralViewer(viewed);
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
                if (isImage && url.isNotEmpty)
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
    final sender = media.senderPubkey.length > 12
        ? '${media.senderPubkey.substring(0, 12)}…'
        : media.senderPubkey;
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

  void _openNewDmDialog() {
    final recipientController = TextEditingController();
    final messageController = TextEditingController();
    showDialog<void>(
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
      await messagingService.sendDM(content, pubkey, activePubkey, '');
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

  void _openNewGroupDmDialog() {
    final messageController = TextEditingController();
    final messagingService = context.read<MessagingService>();
    final sessionService = context.read<SessionService>();
    final partners = messagingService.conversations.keys
        .where((pk) => pk != sessionService.activePubkey)
        .toList()
      ..sort();
    showDialog<void>(
      context: context,
      builder: (dialogContext) => StatefulBuilder(
        builder: (dialogContext, setDialogState) {
          final selected = <String>{};
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
                      child: ListView(
                        shrinkWrap: true,
                        children: partners.map((pk) {
                          return CheckboxListTile(
                            dense: true,
                            value: selected.contains(pk),
                            title: Text(_shortPubkey(pk)),
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
                        }).toList(),
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
          SnackBar(content: Text('Group DM sent (${id.substring(0, 12)}…)')),
        );
      }
      _loadConversations();
    } catch (e) {
      if (dialogContext.mounted) {
        _showSnackBar(dialogContext, 'Send error: $e');
      }
    }
  }

  String _shortPubkey(String pubkey) =>
      pubkey.length > 16 ? '${pubkey.substring(0, 16)}…' : pubkey;

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
          title: const Text('Messages'),
          actions: [
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
        body: Consumer<MessagingService>(
          builder: (context, messagingService, child) {
            final conversations = messagingService.conversations;
            if (conversations.isEmpty &&
                messagingService.pendingEphemeral.isEmpty) {
              return const Center(child: Text('No conversations yet'));
            }

            final convList = conversations.entries.toList();
            return ListView.builder(
              itemExtent: 72.0,
              itemCount: convList.length + 1,
              itemBuilder: (context, index) {
                if (index == 0) {
                  return _ephemeralSection(context, messagingService);
                }
                final entry = convList[index - 1];
                final pubkey = entry.key;
                final messages = entry.value;

                if (messages.isEmpty) return const SizedBox.shrink();

                final lastMessage = messages.last;
                final preview = lastMessage.decrypted
                    ? lastMessage.content
                    : '🔒 ${lastMessage.content}';
                final displayName =
                    pubkey.length >= 16 ? pubkey.substring(0, 16) : pubkey;
                return ListTile(
                  title: Text(displayName),
                  subtitle: Text(preview,
                      maxLines: 1, overflow: TextOverflow.ellipsis),
                  onTap: () {
                    context.go('/inbox/$pubkey');
                  },
                );
              },
            );
          },
        ),
      );
    }

    // Show conversation with specific user
    return Scaffold(
      appBar: AppBar(
        title: Text(
            widget.otherPubkey != null && widget.otherPubkey!.length >= 16
                ? widget.otherPubkey!.substring(0, 16)
                : (widget.otherPubkey ?? 'Messages')),
      ),
      body: Column(
        children: [
          Expanded(
            child: Consumer<MessagingService>(
              builder: (context, messagingService, child) {
                final messages =
                    messagingService.conversations[widget.otherPubkey] ?? [];

                if (messages.isEmpty) {
                  return const Center(child: Text('No messages yet'));
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
                    Expanded(
                      child: TextField(
                        controller: _messageController,
                        decoration: InputDecoration(
                          hintText: 'Message',
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

    return Align(
      alignment: isOwn ? Alignment.centerRight : Alignment.centerLeft,
      child: Container(
        margin: const EdgeInsets.symmetric(vertical: 4, horizontal: 8),
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        decoration: BoxDecoration(
          color: isOwn ? Colors.blue : Colors.grey[300],
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
                  color: isOwn ? Colors.white : Colors.black,
                ),
              ),
            ),
            const SizedBox(height: 4),
            Text(
              _formatTime(message.createdAt),
              style: TextStyle(
                fontSize: 12,
                color: isOwn ? Colors.white70 : Colors.grey,
              ),
            ),
          ],
        ),
      ),
    );
  }

  String _formatTime(int timestamp) {
    final dateTime = DateTime.fromMillisecondsSinceEpoch(timestamp * 1000);
    return '${dateTime.hour}:${dateTime.minute.toString().padLeft(2, '0')}';
  }
}
