import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/groups_service.dart';
import '../services/session_service.dart';
import '../utils/format.dart';

/// Parses a #rrggbb hex color (falls back to neutral gray).
Color hexColor(String hex) {
  final trimmed = hex.trim();
  final match = RegExp(r'^#([0-9a-fA-F]{6})$').hasMatch(trimmed);
  return match
      ? Color(0xFF000000 | int.parse(trimmed.substring(1), radix: 16))
      : const Color(0xFF6b7280);
}

String _shortKey(String pubkey) => firstChars(pubkey, 12);

/// Rooms tab: default `# general` room plus themed rooms with per-room chat.
class GroupRoomsTab extends StatefulWidget {
  /// Rooms tab.
  const GroupRoomsTab({super.key, required this.groupId});

  final String groupId;

  @override
  State<GroupRoomsTab> createState() => _GroupRoomsTabState();
}

class _GroupRoomsTabState extends State<GroupRoomsTab>
    with AutomaticKeepAliveClientMixin {
  static const List<String> _quickEmojis = ['👍', '❤️', '🔥', '😂'];
  static const List<String> _allEmojis = [
    '👍', '❤️', '🔥', '😂', '🎉', '🚀', '👀', '💯',
    '👏', '🙏', '🤯', '😍', '🤔', '😭', '✨', '⚡',
  ];

  String _roomId = '';
  bool _sending = false;
  final _chat = TextEditingController();

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _loadReactions());
  }

  @override
  void dispose() {
    _chat.dispose();
    super.dispose();
  }

  Future<void> _loadReactions() async {
    final me = _me();
    try {
      await context.read<GroupsService>().fetchRoomReactions(
            widget.groupId,
            _roomId,
            viewerPubkey: me,
          );
      if (mounted) setState(() {});
    } catch (e) {
      debugPrint('room reactions: $e');
    }
  }

  Future<void> _switchRoom(String roomId) async {
    setState(() => _roomId = roomId);
    try {
      final groups = context.read<GroupsService>();
      await groups.fetchMessages(widget.groupId, roomId: roomId);
      final me = _me();
      await groups.fetchRoomReactions(widget.groupId, roomId, viewerPubkey: me);
      if (mounted) setState(() {});
    } catch (e) {
      debugPrint('room messages: $e');
    }
  }

  Future<void> _send() async {
    final text = _chat.text.trim();
    if (text.isEmpty) return;
    setState(() => _sending = true);
    try {
      final groups = context.read<GroupsService>();
      await groups.postMessage(widget.groupId, text, roomId: _roomId);
      _chat.clear();
      if (!mounted) return;
      await groups.fetchMessages(widget.groupId, roomId: _roomId);
      final me = _me();
      await groups.fetchRoomReactions(widget.groupId, _roomId, viewerPubkey: me);
      if (mounted) setState(() {});
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Send failed: $e')));
      }
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  Future<void> _react(String messageId, String emoji) async {
    final me = _me();
    if (me == null) return;
    final groups = context.read<GroupsService>();
    try {
      await groups.reactToRoomMessage(
        widget.groupId,
        _roomId,
        messageId,
        emoji,
        me,
      );
      await groups.fetchRoomReactions(widget.groupId, _roomId, viewerPubkey: me);
      if (!mounted) return;
      setState(() {});
    } catch (e) {
      if (!context.mounted) return;
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: SelectableText('React failed: $e')));
    }
  }

  Future<void> _showEmojiPicker(String messageId) async {
    final me = _me();
    if (me == null) return;
    final emoji = await showModalBottomSheet<String>(
      context: context,
      builder: (context) => Padding(
        padding: const EdgeInsets.all(16),
        child: Wrap(
          spacing: 16,
          runSpacing: 8,
          children: [
            for (final e in _allEmojis)
              IconButton(
                icon: Text(e, style: const TextStyle(fontSize: 24)),
                onPressed: () => Navigator.pop(context, e),
              ),
          ],
        ),
      ),
    );
    if (emoji == null || !mounted) return;
    await _react(messageId, emoji);
  }

  Widget _reactionRow(String messageId, GroupsService api) {
    final reactions = api.roomReactionsFor(messageId);
    final displayedEmojis = <String>{..._quickEmojis};
    for (final r in reactions) {
      displayedEmojis.add(r.emoji);
    }

    return Wrap(
      spacing: 4,
      runSpacing: 4,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        for (final emoji in displayedEmojis)
          Builder(builder: (context) {
            final match = reactions.firstWhere(
              (r) => r.emoji == emoji,
              orElse: () => RoomReaction(
                messageId: messageId,
                emoji: emoji,
                count: 0,
                reacted: false,
              ),
            );
            return FilterChip(
              avatar: Text(emoji, style: const TextStyle(fontSize: 12)),
              label: Text('${match.count}'),
              selected: match.reacted,
              showCheckmark: false,
              visualDensity: VisualDensity.compact,
              materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
              onSelected: (_) => _react(messageId, emoji),
            );
          }),
        ActionChip(
          label: const Icon(Icons.add_reaction_outlined, size: 16),
          visualDensity: VisualDensity.compact,
          tooltip: 'More emoji',
          onPressed: () => _showEmojiPicker(messageId),
        ),
      ],
    );
  }

  Future<void> _roomDialog({GroupRoom? room}) async {
    final isOwner = _isOwner();
    if (!isOwner) return;
    final name = TextEditingController(text: room?.name ?? '');
    final topic = TextEditingController(text: room?.topic ?? '');
    final emoji = TextEditingController(text: room?.emoji ?? '');
    var color = hexColor(room?.color ?? '#8b5cf6');

    await showDialog<void>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) => AlertDialog(
          title: Text(room == null ? 'New room' : 'Edit room'),
          content: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextField(
                  controller: name,
                  decoration: const InputDecoration(labelText: 'Name *'),
                ),
                TextField(
                  controller: topic,
                  decoration: const InputDecoration(labelText: 'Topic'),
                ),
                TextField(
                  controller: emoji,
                  decoration: const InputDecoration(labelText: 'Emoji'),
                ),
                const SizedBox(height: 8),
                const Text('Color',
                    style: TextStyle(fontWeight: FontWeight.w600)),
                const SizedBox(height: 6),
                Wrap(
                  spacing: 6,
                  runSpacing: 6,
                  children: [
                    for (final c in groupRoleColors)
                      GestureDetector(
                        onTap: () => setDialogState(() => color = hexColor(c)),
                        child: Container(
                          width: 26,
                          height: 26,
                          decoration: BoxDecoration(
                            color: hexColor(c),
                            shape: BoxShape.circle,
                            border: Border.all(
                              width: 2,
                              color: color == hexColor(c)
                                  ? Colors.white
                                  : Colors.transparent,
                            ),
                          ),
                        ),
                      ),
                  ],
                ),
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () async {
                final n = name.text.trim();
                if (n.isEmpty) return;
                final t = topic.text.trim();
                final em = emoji.text.trim();
                final api = context.read<GroupsService>();
                final me = _me();
                Navigator.pop(context);
                final hex = _hex(color);
                if (me == null) return;
                try {
                  if (room == null) {
                    await api.createRoom(widget.groupId, n, t, em, hex, me);
                  } else {
                    await api.updateRoom(
                        room.id, widget.groupId, n, t, em, hex, me);
                  }
                } catch (e) {
                  if (!context.mounted) return;
                  ScaffoldMessenger.of(context).showSnackBar(SnackBar(
                      content: SelectableText('Room save failed: $e')));
                }
              },
              child: const Text('Save'),
            ),
          ],
        ),
      ),
    );
    name.dispose();
    topic.dispose();
    emoji.dispose();
  }

  String _hex(Color c) =>
      '#${(c.toARGB32() & 0xFFFFFF).toRadixString(16).padLeft(6, '0')}';

  String? _me() => context.read<SessionService>().activePubkey;

  bool _isOwner() {
    final g = context.read<GroupsService>().current;
    return g != null && g.owner == _me();
  }

  Future<void> _deleteRoom(GroupRoom room) async {
    final me = _me();
    if (me == null) return;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Delete room?'),
        content: Text('Delete "${room.name}"? Its messages move to # general.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            style: FilledButton.styleFrom(
              backgroundColor: Theme.of(context).colorScheme.error,
            ),
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    try {
      await context.read<GroupsService>().deleteRoom(room.id, me);
      if (_roomId == room.id) await _switchRoom('');
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Delete failed: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final api = context.watch<GroupsService>();
    final isOwner = _isOwner();
    final rooms = api.rooms;
    return Column(
      children: [
        Container(
          height: 48,
          padding: const EdgeInsets.symmetric(horizontal: 12),
          child: ListView(
            scrollDirection: Axis.horizontal,
            children: [
              ChoiceChip(
                avatar: const Icon(Icons.tag, size: 16),
                label: const Text('general'),
                selected: _roomId.isEmpty,
                onSelected: (_) => _switchRoom(''),
              ),
              const SizedBox(width: 8),
              ChoiceChip(
                avatar: const Icon(Icons.volume_up_outlined, size: 16),
                label: const Text('Voice Lounge'),
                selected: _roomId == 'voice_room',
                onSelected: (_) => _switchRoom('voice_room'),
              ),
              const SizedBox(width: 8),
              ChoiceChip(
                avatar: const Icon(Icons.podcasts_outlined, size: 16),
                label: const Text('Stage Channel'),
                selected: _roomId == 'stage_channel',
                onSelected: (_) => _switchRoom('stage_channel'),
              ),
              for (final room in rooms) ...[
                const SizedBox(width: 8),
                GestureDetector(
                  onLongPress: isOwner
                      ? () => showModalBottomSheet<void>(
                            context: context,
                            builder: (context) => SafeArea(
                              child: Column(
                                mainAxisSize: MainAxisSize.min,
                                children: [
                                  ListTile(
                                    leading: const Icon(Icons.edit_outlined),
                                    title: const Text('Edit room'),
                                    onTap: () {
                                      Navigator.pop(context);
                                      _roomDialog(room: room);
                                    },
                                  ),
                                  ListTile(
                                    leading: const Icon(Icons.delete_outline),
                                    title: const Text('Delete room'),
                                    onTap: () {
                                      Navigator.pop(context);
                                      _deleteRoom(room);
                                    },
                                  ),
                                ],
                              ),
                            ),
                          )
                      : null,
                  child: ChoiceChip(
                    label: Text(
                        '${room.emoji.isNotEmpty ? '${room.emoji} ' : ''}${room.name}'),
                    selected: _roomId == room.id,
                    onSelected: (_) => _switchRoom(room.id),
                  ),
                ),
              ],
              const SizedBox(width: 8),
              if (isOwner)
                IconButton(
                  icon: const Icon(Icons.add),
                  tooltip: 'New room',
                  onPressed: () => _roomDialog(),
                ),
            ],
          ),
        ),
        const Divider(height: 1),
        if (_roomId == 'stage_channel')
          Expanded(
            child: Center(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  const Icon(Icons.podcasts,
                      size: 64, color: Colors.purpleAccent),
                  const SizedBox(height: 12),
                  const Text('Stage Channel Live',
                      style:
                          TextStyle(fontSize: 18, fontWeight: FontWeight.bold)),
                  const SizedBox(height: 6),
                  const Text(
                      'Listen to speakers or raise hand to speak on stage',
                      style: TextStyle(color: Colors.grey)),
                  const SizedBox(height: 16),
                  FilledButton.icon(
                    icon: const Icon(Icons.front_hand),
                    label: const Text('Raise Hand to Speak'),
                    onPressed: () {
                      ScaffoldMessenger.of(context).showSnackBar(
                        const SnackBar(
                            content: Text(
                                'Hand raised! Waiting for stage mod approval.')),
                      );
                    },
                  ),
                ],
              ),
            ),
          )
        else if (_roomId == 'voice_room')
          Expanded(
            child: Center(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  const Icon(Icons.volume_up, size: 64, color: Colors.green),
                  const SizedBox(height: 12),
                  const Text('Voice Lounge Connected',
                      style:
                          TextStyle(fontSize: 18, fontWeight: FontWeight.bold)),
                  const SizedBox(height: 6),
                  const Text('WebRTC voice mesh active · 0 members speaking',
                      style: TextStyle(color: Colors.grey)),
                  const SizedBox(height: 16),
                  FilledButton.tonalIcon(
                    icon: const Icon(Icons.mic_off),
                    label: const Text('Mute Audio'),
                    onPressed: () {},
                  ),
                ],
              ),
            ),
          )
        else
          Expanded(
            child: api.messages.isEmpty
                ? const Center(child: Text('No messages yet'))
                : ListView.builder(
                    itemCount: api.messages.length,
                    itemBuilder: (context, i) {
                      final m = api.messages[api.messages.length - 1 - i];
                      return Padding(
                        padding: const EdgeInsets.symmetric(
                            horizontal: 12, vertical: 6),
                        child: Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            const CircleAvatar(
                              radius: 14,
                              child: Icon(Icons.person_outline, size: 16),
                            ),
                            const SizedBox(width: 10),
                            Expanded(
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Row(
                                    children: [
                                      Text(
                                        _shortKey(m.senderPubkey),
                                        style: const TextStyle(
                                          fontSize: 12,
                                          fontWeight: FontWeight.bold,
                                        ),
                                      ),
                                      if (m.createdAt > 0) ...[
                                        const SizedBox(width: 6),
                                        Text(
                                          formatTimestamp(m.createdAt),
                                          style: const TextStyle(
                                            fontSize: 11,
                                            color: Colors.grey,
                                          ),
                                        ),
                                      ],
                                    ],
                                  ),
                                  const SizedBox(height: 2),
                                  Text(m.content),
                                  const SizedBox(height: 4),
                                  _reactionRow(m.id, api),
                                ],
                              ),
                            ),
                          ],
                        ),
                      );
                    },
                  ),
          ),
        Container(
          padding: const EdgeInsets.all(8),
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _chat,
                  decoration: InputDecoration(
                    hintText: 'Message ${_roomId.isEmpty ? '# general' : ''}',
                    border: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(24),
                    ),
                    contentPadding: const EdgeInsets.symmetric(
                      horizontal: 16,
                      vertical: 8,
                    ),
                  ),
                  enabled: !_sending,
                ),
              ),
              const SizedBox(width: 8),
              IconButton(
                icon: const Icon(Icons.send),
                onPressed: _sending ? null : _send,
              ),
            ],
          ),
        ),
      ],
    );
  }
}

/// Threads tab: reddit-like thread list with a nested-reply detail view.
class GroupThreadsTab extends StatefulWidget {
  /// Threads tab.
  const GroupThreadsTab({super.key, required this.groupId});

  final String groupId;

  @override
  State<GroupThreadsTab> createState() => _GroupThreadsTabState();
}

class _GroupThreadsTabState extends State<GroupThreadsTab>
    with AutomaticKeepAliveClientMixin {
  static const List<String> _quickEmojis = ['👍', '🔥', '😂'];
  static const List<String> _replyEmojis = ['👍', '❤️', '🔥', '😂'];
  static const List<String> _allEmojis = ['👍', '❤️', '😂', '😮', '😢', '😡'];

  String? _openThreadId;
  String _replyParent = '';
  bool _sendingReply = false;
  final _title = TextEditingController();
  final _body = TextEditingController();
  final _reply = TextEditingController();

  @override
  bool get wantKeepAlive => true;

  @override
  void dispose() {
    _title.dispose();
    _body.dispose();
    _reply.dispose();
    super.dispose();
  }

  /// Fetch reaction summaries for every listed thread (list cards + detail
  /// rows both read the per-thread cache).
  Future<void> _changeSort(ThreadSort sort, GroupsService api) async {
    if (api.threadSort == sort) return;
    api.threadSort = sort;
    try {
      await api.fetchThreads(widget.groupId);
    } catch (e) {
      debugPrint('thread sort: $e');
    }
    if (mounted) setState(() {});
  }

  Future<void> _react(String targetId, String replyId, String emoji) async {
    final me = context.read<SessionService>().activePubkey;
    final groups = context.read<GroupsService>();
    if (me == null) return;
    try {
      await groups.react(targetId, replyId, emoji, me);
      await groups.fetchThreads(widget.groupId);
      await groups.fetchReactions(targetId, me);
      if (!mounted) return;
      setState(() {});
    } catch (e) {
      if (!context.mounted) return;
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: SelectableText('React failed: $e')));
    }
  }

  /// Emoji picker bottom sheet (same set as the feed), then toggles the
  /// chosen emoji on [targetId] (thread id with empty [replyId]).
  Future<void> _showEmojiPicker(String targetId, String replyId) async {
    final me = context.read<SessionService>().activePubkey;
    if (me == null) return;
    final emoji = await showModalBottomSheet<String>(
      context: context,
      builder: (context) => Padding(
        padding: const EdgeInsets.all(16),
        child: Wrap(
          spacing: 16,
          runSpacing: 8,
          children: [
            for (final e in _allEmojis)
              IconButton(
                icon: Text(e, style: const TextStyle(fontSize: 24)),
                onPressed: () => Navigator.pop(context, e),
              ),
          ],
        ),
      ),
    );
    if (emoji == null || !mounted) return;
    await _react(targetId, replyId, emoji);
  }

  /// Heart-like toggle button (feed parity): single-tap ❤️ with count.
  Widget _heartButton(String targetId, GroupsService api) {
    final hearts =
        api.reactionsFor(targetId).where((r) => r.emoji == '❤️').toList();
    final count = hearts.fold(0, (s, r) => s + r.count);
    final reacted = hearts.any((r) => r.reacted);
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        IconButton(
          icon: Icon(
            reacted ? Icons.favorite : Icons.favorite_border,
            size: 18,
            color: reacted ? Colors.red : null,
          ),
          visualDensity: VisualDensity.compact,
          onPressed: () => _react(targetId, '', '❤️'),
        ),
        Text('$count',
            style: const TextStyle(fontSize: 11, color: Colors.grey)),
      ],
    );
  }

  /// Emoji chip row for one target (thread id with empty [replyId], else
  /// reply id). Always visible so reactions start from zero; [quick] chips
  /// toggle directly, the ＋ chip opens the full emoji picker.
  Widget _reactionRow(String targetId, String replyId, GroupsService api,
      {List<String> quick = _quickEmojis}) {
    return Wrap(
      spacing: 4,
      runSpacing: 4,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        for (final emoji in quick)
          Builder(builder: (context) {
            final rs = api
                .reactionsFor(targetId)
                .where((r) => r.emoji == emoji)
                .toList();
            final count = rs.fold(0, (s, r) => s + r.count);
            final reacted = rs.any((r) => r.reacted);
            return FilterChip(
              avatar: Text(emoji, style: const TextStyle(fontSize: 12)),
              label: Text('$count'),
              selected: reacted,
              showCheckmark: false,
              visualDensity: VisualDensity.compact,
              materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
              onSelected: (_) => _react(targetId, replyId, emoji),
            );
          }),
        ActionChip(
          label: const Icon(Icons.add_reaction_outlined, size: 16),
          visualDensity: VisualDensity.compact,
          tooltip: 'More emoji',
          onPressed: () => _showEmojiPicker(targetId, replyId),
        ),
      ],
    );
  }

  Future<void> _openThread(GroupThread t) async {
    setState(() {
      _openThreadId = t.id;
      _replyParent = '';
    });
    try {
      await context.read<GroupsService>().fetchReplies(t.id);
      if (mounted) setState(() {});
    } catch (e) {
      debugPrint('thread replies: $e');
    }
  }

  Future<void> _createThread() async {
    final me = context.read<SessionService>().activePubkey;
    if (me == null) return;
    await showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('New thread'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: _title,
              decoration: const InputDecoration(labelText: 'Title *'),
            ),
            TextField(
              controller: _body,
              maxLines: 4,
              decoration: const InputDecoration(labelText: 'Body'),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () async {
              final t = _title.text.trim();
              if (t.isEmpty) return;
              Navigator.pop(context);
              try {
                await context
                    .read<GroupsService>()
                    .createThread(widget.groupId, t, _body.text.trim(), me);
                _title.clear();
                _body.clear();
              } catch (e) {
                if (context.mounted) {
                  ScaffoldMessenger.of(context).showSnackBar(
                      SnackBar(content: SelectableText('Create failed: $e')));
                }
              }
            },
            child: const Text('Create'),
          ),
        ],
      ),
    );
  }

  Future<void> _sendReply() async {
    final text = _reply.text.trim();
    if (text.isEmpty) return;
    final me = context.read<SessionService>().activePubkey;
    final threadId = _openThreadId;
    if (me == null || threadId == null) return;
    setState(() => _sendingReply = true);
    try {
      final api = context.read<GroupsService>();
      await api.replyToThread(threadId, _replyParent, text, me);
      _reply.clear();
      if (!mounted) return;
      setState(() => _replyParent = '');
      await api.fetchThreads(widget.groupId);
      if (mounted) setState(() {});
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Reply failed: $e')));
      }
    } finally {
      if (mounted) setState(() => _sendingReply = false);
    }
  }

  Future<void> _pinThread(GroupThread t, bool pinned) async {
    final me = context.read<SessionService>().activePubkey;
    final groups = context.read<GroupsService>();
    final messenger = ScaffoldMessenger.of(context);
    if (me == null) return;
    try {
      await groups.setThreadPinned(t.id, pinned, me);
      await groups.fetchThreads(widget.groupId);
      if (mounted) setState(() {});
    } catch (e) {
      messenger
          .showSnackBar(SnackBar(content: SelectableText('Pin failed: $e')));
    }
  }

  Future<void> _deleteThread(GroupThread t) async {
    final me = context.read<SessionService>().activePubkey;
    if (me == null) return;
    final messenger = ScaffoldMessenger.of(context);
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Delete thread?'),
        content: Text('Delete "${t.title}" and all its replies?'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            style: FilledButton.styleFrom(
              backgroundColor: Theme.of(context).colorScheme.error,
            ),
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    final groups = context.read<GroupsService>();
    try {
      await groups.deleteThread(t.id, me);
      if (_openThreadId == t.id) setState(() => _openThreadId = null);
      await groups.fetchThreads(widget.groupId);
      if (mounted) setState(() {});
    } catch (e) {
      messenger
          .showSnackBar(SnackBar(content: SelectableText('Delete failed: $e')));
    }
  }

  Widget _threadCard(GroupThread t, bool isOwner, GroupsService api) {
    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
      child: InkWell(
        onTap: () => _openThread(t),
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  if (t.isPinned) ...[
                    Icon(Icons.push_pin,
                        size: 16, color: Theme.of(context).colorScheme.primary),
                    const SizedBox(width: 6),
                  ],
                  Expanded(
                    child: Text(t.title,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(fontWeight: FontWeight.w600)),
                  ),
                  if (isOwner)
                    PopupMenuButton<String>(
                      icon: const Icon(Icons.more_vert, size: 18),
                      onSelected: (v) {
                        if (v == 'pin') _pinThread(t, !t.isPinned);
                        if (v == 'delete') _deleteThread(t);
                      },
                      itemBuilder: (context) => [
                        PopupMenuItem(
                          value: 'pin',
                          child: Text(t.isPinned ? 'Unpin' : 'Pin'),
                        ),
                        const PopupMenuItem(
                            value: 'delete', child: Text('Delete')),
                      ],
                    ),
                ],
              ),
              if (t.body.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: 4),
                  child: Text(t.body,
                      maxLines: 3,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(fontSize: 13)),
                ),
              const SizedBox(height: 6),
              Text(
                '${_shortKey(t.author)} · ${t.replyCount} replies'
                '${t.reactionCount > 0 ? ' · ${t.reactionCount} reactions' : ''}',
                style: Theme.of(context)
                    .textTheme
                    .bodySmall
                    ?.copyWith(color: Theme.of(context).hintColor),
              ),
              Padding(
                padding: const EdgeInsets.only(top: 6),
                child: Wrap(
                  spacing: 8,
                  runSpacing: 4,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    _heartButton(t.id, api),
                    _reactionRow(t.id, '', api),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _replyTile(GroupThreadReply r, GroupsService api) {
    final nested = r.parentId.isNotEmpty;
    return Padding(
      padding: EdgeInsets.only(left: nested ? 24 : 0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          ListTile(
            dense: true,
            leading: const Icon(Icons.person_outline, size: 20),
            title:
                Text(_shortKey(r.author), style: const TextStyle(fontSize: 12)),
            subtitle: Text(r.content),
            trailing: TextButton(
              onPressed: () => setState(() => _replyParent = r.id),
              child: const Text('Reply', style: TextStyle(fontSize: 12)),
            ),
            isThreeLine: true,
          ),
          Padding(
            padding: const EdgeInsets.only(left: 16, bottom: 4),
            child: _reactionRow(r.id, r.id, api, quick: _replyEmojis),
          ),
        ],
      ),
    );
  }

  Widget _threadDetail(GroupThread t, GroupsService api) {
    final me = context.read<SessionService>().activePubkey;
    final isOwner = api.current != null && api.current!.owner == me;
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  IconButton(
                    icon: const Icon(Icons.arrow_back),
                    tooltip: 'Back to threads',
                    onPressed: () => setState(() => _openThreadId = null),
                  ),
                  Expanded(
                    child: Text(t.title,
                        style: Theme.of(context).textTheme.titleMedium),
                  ),
                  if (isOwner)
                    PopupMenuButton<String>(
                      onSelected: (v) {
                        if (v == 'pin') _pinThread(t, !t.isPinned);
                        if (v == 'delete') {
                          _deleteThread(t);
                        }
                      },
                      itemBuilder: (context) => [
                        PopupMenuItem(
                          value: 'pin',
                          child: Text(t.isPinned ? 'Unpin' : 'Pin'),
                        ),
                        const PopupMenuItem(
                            value: 'delete', child: Text('Delete')),
                      ],
                    ),
                ],
              ),
              if (t.body.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(bottom: 4),
                  child: SelectableText(t.body),
                ),
              Text(
                '${_shortKey(t.author)} · ${t.replyCount} replies'
                '${t.reactionCount > 0 ? ' · ${t.reactionCount} reactions' : ''}',
                style: Theme.of(context)
                    .textTheme
                    .bodySmall
                    ?.copyWith(color: Theme.of(context).hintColor),
              ),
              Padding(
                padding: const EdgeInsets.only(top: 6),
                child: Wrap(
                  spacing: 8,
                  runSpacing: 4,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    _heartButton(t.id, api),
                    _reactionRow(t.id, '', api),
                  ],
                ),
              ),
            ],
          ),
        ),
        const Divider(height: 1),
        Expanded(
          child: api.replies.isEmpty
              ? const Center(child: Text('No replies yet'))
              : ListView.builder(
                  itemCount: api.replies.length,
                  itemBuilder: (context, i) => _replyTile(api.replies[i], api),
                ),
        ),
        Container(
          padding: const EdgeInsets.all(8),
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _reply,
                  decoration: InputDecoration(
                    hintText: _replyParent.isEmpty
                        ? 'Reply to thread'
                        : 'Reply to a reply',
                    border: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(24),
                    ),
                    contentPadding: const EdgeInsets.symmetric(
                      horizontal: 16,
                      vertical: 8,
                    ),
                  ),
                  enabled: !_sendingReply,
                ),
              ),
              const SizedBox(width: 8),
              IconButton(
                icon: const Icon(Icons.send),
                onPressed: _sendingReply ? null : _sendReply,
              ),
            ],
          ),
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final api = context.watch<GroupsService>();
    final g = api.current;
    final isOwner =
        g != null && g.owner == context.read<SessionService>().activePubkey;
    final open = _openThreadId == null
        ? null
        : api.threads.where((t) => t.id == _openThreadId).firstOrNull;
    if (open != null) return _threadDetail(open, api);
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
          child: Row(
            children: [
              Text('Threads (${api.threads.length})',
                  style: Theme.of(context).textTheme.titleSmall),
              const Spacer(),
              FilledButton.tonalIcon(
                onPressed: _createThread,
                icon: const Icon(Icons.add, size: 18),
                label: const Text('New Thread'),
              ),
            ],
          ),
        ),
        Padding(
          padding: const EdgeInsets.only(left: 12, right: 12, bottom: 8),
          child: Row(
            children: [
              SegmentedButton<ThreadSort>(
                segments: const [
                  ButtonSegment(
                    value: ThreadSort.popular,
                    icon: Icon(Icons.local_fire_department_outlined, size: 16),
                    label: Text('Popular'),
                  ),
                  ButtonSegment(
                    value: ThreadSort.newest,
                    icon: Icon(Icons.schedule, size: 16),
                    label: Text('Newest'),
                  ),
                ],
                selected: {api.threadSort},
                showSelectedIcon: false,
                style: const ButtonStyle(
                  visualDensity: VisualDensity.compact,
                  tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                ),
                onSelectionChanged: (s) => _changeSort(s.first, api),
              ),
              const Spacer(),
              Text(
                api.threadSort == ThreadSort.popular
                    ? 'hot: reactions + replies / hour'
                    : 'pinned first, newest first',
                style: Theme.of(context)
                    .textTheme
                    .bodySmall
                    ?.copyWith(color: Theme.of(context).hintColor),
              ),
            ],
          ),
        ),
        Expanded(
          child: api.threads.isEmpty
              ? const Center(child: Text('No threads yet'))
              : ListView.builder(
                  itemCount: api.threads.length,
                  itemBuilder: (context, i) =>
                      _threadCard(api.threads[i], isOwner, api),
                ),
        ),
      ],
    );
  }
}

/// Voice channels tab: channel list with local presence; audio transport is
/// a roadmap surface (honest gating).
class GroupVoiceTab extends StatefulWidget {
  /// Voice channels tab.
  const GroupVoiceTab({super.key, required this.groupId});

  final String groupId;

  @override
  State<GroupVoiceTab> createState() => _GroupVoiceTabState();
}

class _GroupVoiceTabState extends State<GroupVoiceTab>
    with AutomaticKeepAliveClientMixin {
  final Map<String, List<GroupVoicePresence>> _presence = {};
  bool _presenceAttempted = false;
  final _name = TextEditingController();

  @override
  bool get wantKeepAlive => true;

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  Future<void> _loadPresence(GroupsService api) async {
    for (final ch in api.voiceChannels) {
      try {
        _presence[ch.id] = await api.fetchPresence(ch.id);
      } catch (e) {
        debugPrint('voice presence: $e');
      }
    }
    if (mounted) setState(() {});
  }

  Future<void> _join(GroupVoiceChannel ch) async {
    final me = context.read<SessionService>().activePubkey;
    final groups = context.read<GroupsService>();
    final messenger = ScaffoldMessenger.of(context);
    if (me == null) return;
    try {
      await groups.voiceJoin(ch.id, me);
      if (mounted) {
        messenger.showSnackBar(const SnackBar(
            content: SelectableText(
                'Voice transport unavailable (roadmap) — presence recorded')));
      }
      await _loadPresence(groups);
    } catch (e) {
      messenger
          .showSnackBar(SnackBar(content: SelectableText('Join failed: $e')));
    }
  }

  Future<void> _leave(GroupVoiceChannel ch) async {
    final me = context.read<SessionService>().activePubkey;
    final groups = context.read<GroupsService>();
    final messenger = ScaffoldMessenger.of(context);
    if (me == null) return;
    try {
      await groups.voiceLeave(ch.id, me);
      await _loadPresence(groups);
    } catch (e) {
      messenger
          .showSnackBar(SnackBar(content: SelectableText('Leave failed: $e')));
    }
  }

  Future<void> _createChannel() async {
    final me = context.read<SessionService>().activePubkey;
    if (me == null) return;
    await showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('New voice channel'),
        content: TextField(
          controller: _name,
          decoration: const InputDecoration(labelText: 'Name *'),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () async {
              final n = _name.text.trim();
              if (n.isEmpty) return;
              Navigator.pop(context);
              final groups = context.read<GroupsService>();
              try {
                await groups.createVoiceChannel(widget.groupId, n, me);
                _name.clear();
                await _loadPresence(groups);
              } catch (e) {
                if (context.mounted) {
                  ScaffoldMessenger.of(context).showSnackBar(
                      SnackBar(content: SelectableText('Create failed: $e')));
                }
              }
            },
            child: const Text('Create'),
          ),
        ],
      ),
    );
  }

  Future<void> _deleteChannel(GroupVoiceChannel ch) async {
    final me = context.read<SessionService>().activePubkey;
    if (me == null) return;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Delete channel?'),
        content: Text('Delete "${ch.name}"?'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            style: FilledButton.styleFrom(
              backgroundColor: Theme.of(context).colorScheme.error,
            ),
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    try {
      await context.read<GroupsService>().deleteVoiceChannel(ch.id, me);
      _presence.remove(ch.id);
      if (mounted) setState(() {});
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Delete failed: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final api = context.watch<GroupsService>();
    final me = context.read<SessionService>().activePubkey;
    final g = api.current;
    final isOwner = g != null && g.owner == me;
    if (!_presenceAttempted && api.voiceChannels.isNotEmpty) {
      _presenceAttempted = true;
      _loadPresence(api);
    }
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
          child: Row(
            children: [
              Text('Voice channels (${api.voiceChannels.length})',
                  style: Theme.of(context).textTheme.titleSmall),
              const Spacer(),
              if (isOwner)
                IconButton(
                  icon: const Icon(Icons.add),
                  tooltip: 'New voice channel',
                  onPressed: _createChannel,
                ),
            ],
          ),
        ),
        Expanded(
          child: api.voiceChannels.isEmpty
              ? const Center(
                  child: Padding(
                    padding: EdgeInsets.all(24),
                    child: Text(
                      'No voice channels yet.\n\nVoice media transport is on the '
                      'roadmap — joining records local presence only.',
                      textAlign: TextAlign.center,
                      style: TextStyle(color: Colors.grey),
                    ),
                  ),
                )
              : ListView.builder(
                  itemCount: api.voiceChannels.length,
                  itemBuilder: (context, i) {
                    final ch = api.voiceChannels[i];
                    final present = _presence[ch.id] ?? const [];
                    final joined = present.any((p) => p.pubkey == me);
                    return ListTile(
                      leading: const Icon(Icons.headphones_outlined),
                      title: Text(ch.name),
                      subtitle: Text('${present.length} here'),
                      trailing: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          if (joined)
                            FilledButton.tonal(
                              onPressed: () => _leave(ch),
                              child: const Text('Leave'),
                            )
                          else
                            FilledButton(
                              onPressed: () => _join(ch),
                              child: const Text('Join'),
                            ),
                          if (isOwner)
                            IconButton(
                              icon: const Icon(Icons.delete_outline),
                              tooltip: 'Delete channel',
                              onPressed: () => _deleteChannel(ch),
                            ),
                        ],
                      ),
                    );
                  },
                ),
        ),
      ],
    );
  }
}
