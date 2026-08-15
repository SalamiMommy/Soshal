import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/dating_service.dart';
import '../services/session_service.dart';
import '../utils/format.dart';

/// Dating: browse cards, like/pass/superlike, matches, likes received.
class DatingScreen extends StatefulWidget {
  /// Dating screen.
  const DatingScreen({super.key});

  @override
  State<DatingScreen> createState() => _DatingScreenState();
}

class _DatingScreenState extends State<DatingScreen>
    with SingleTickerProviderStateMixin {
  late final TabController _tabs;
  int _cardIndex = 0;
  bool _loading = true;
  bool _hasProfile = false;
  DatingCard? _matchCard;
  final Map<String, Future<double>> _scores = {};

  @override
  void initState() {
    super.initState();
    _tabs = TabController(length: 3, vsync: this);
    _load();
  }

  @override
  void dispose() {
    _tabs.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final session = context.read<SessionService>();
      final api = context.read<DatingService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) return;
      try {
        await api.getOwnProfile(pubkey);
        _hasProfile = true;
      } catch (_) {
        _hasProfile = false;
      }
      await api.fetchProfiles(pubkey);
    } catch (e) {
      debugPrint('dating load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  @override
  Widget build(BuildContext context) {
    final session = context.watch<SessionService>();
    final pubkey = session.activePubkey;

    if (pubkey == null) {
      return Scaffold(
        appBar: AppBar(title: const Text('Dating')),
        body: const Center(child: Text('Sign in to use Dating')),
      );
    }

    return Scaffold(
      appBar: AppBar(
        title: const Text('Dating'),
        actions: [
          IconButton(
            icon: const Icon(Icons.tune),
            tooltip: 'Filter',
            onPressed: () => _showFilterDialog(pubkey),
          ),
          IconButton(
            icon: const Icon(Icons.person_add_alt),
            tooltip: 'My dating profile',
            onPressed: () => context.push('/dating/me'),
          ),
        ],
      ),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : !_hasProfile
              ? Center(
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      const Text('Create your dating profile to start'),
                      const SizedBox(height: 16),
                      FilledButton(
                        onPressed: () => context.push('/dating/me'),
                        child: const Text('Create profile'),
                      ),
                    ],
                  ),
                )
              : Stack(
                  children: [
                    Column(
                      children: [
                        TabBar(
                          controller: _tabs,
                          tabs: const [
                            Tab(text: 'Browse'),
                            Tab(text: 'Matches'),
                            Tab(text: 'Likes'),
                          ],
                        ),
                        Expanded(
                          child: TabBarView(
                            controller: _tabs,
                            children: [
                              _buildBrowse(pubkey),
                              _buildMatches(pubkey),
                              _buildLikes(pubkey),
                            ],
                          ),
                        ),
                      ],
                    ),
                    if (_matchCard != null) _buildMatchOverlay(pubkey),
                  ],
                ),
    );
  }

  Future<void> _showFilterDialog(String pubkey) async {
    final minAge = TextEditingController();
    final maxAge = TextEditingController();
    final interests = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Filter profiles'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: minAge,
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(labelText: 'Min age'),
            ),
            TextField(
              controller: maxAge,
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(labelText: 'Max age'),
            ),
            TextField(
              controller: interests,
              decoration: const InputDecoration(
                labelText: 'Interests',
                hintText: 'comma separated',
              ),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Clear'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Apply'),
          ),
        ],
      ),
    );
    if (ok == null || !mounted) return;
    final api = context.read<DatingService>();
    if (!ok) {
      await api.fetchProfiles(pubkey);
      return;
    }
    final list = interests.text
        .split(',')
        .map((e) => e.trim())
        .where((e) => e.isNotEmpty)
        .toList();
    await api.filterProfiles(
      pubkey,
      minAge: int.tryParse(minAge.text.trim()) ?? 0,
      maxAge: int.tryParse(maxAge.text.trim()) ?? 0,
      interests: list,
    );
  }

  Widget _buildBrowse(String pubkey) {
    return Consumer<DatingService>(
      builder: (context, api, _) {
        if (api.cards.isEmpty) {
          return const Center(child: Text('No profiles nearby yet'));
        }
        final card = api.cards[_cardIndex.clamp(0, api.cards.length - 1)];
        return Column(
          children: [
            Expanded(
              child: ListView(
                padding: const EdgeInsets.all(16),
                children: [
                  Card(
                    clipBehavior: Clip.antiAlias,
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        if (card.images.isNotEmpty)
                          Image.network(
                            card.images.first,
                            height: 220,
                            cacheHeight: 440,
                            fit: BoxFit.cover,
                            errorBuilder: (_, __, ___) => Container(
                              height: 220,
                              color: Colors.grey[300],
                              child: const Icon(Icons.person, size: 80),
                            ),
                          )
                        else
                          Container(
                            height: 220,
                            color: Colors.grey[300],
                            child: const Icon(Icons.person, size: 80),
                          ),
                        Padding(
                          padding: const EdgeInsets.all(16),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Row(
                                children: [
                                  Text(
                                    card.name.isEmpty
                                        ? firstChars(card.pubkey, 12)
                                        : card.name,
                                    style:
                                        Theme.of(context).textTheme.titleLarge,
                                  ),
                                  if (card.age > 0) ...[
                                    const SizedBox(width: 8),
                                    Text(
                                      '${card.age}',
                                      style: Theme.of(context)
                                          .textTheme
                                          .titleLarge,
                                    ),
                                  ],
                                  const Spacer(),
                                  FutureBuilder<double>(
                                    future: _scoreFor(api, pubkey, card.pubkey),
                                    builder: (context, snapshot) {
                                      final score = snapshot.data ?? 0;
                                      if (!snapshot.hasData || score <= 0) {
                                        return const SizedBox.shrink();
                                      }
                                      return Container(
                                        padding: const EdgeInsets.symmetric(
                                            horizontal: 10, vertical: 4),
                                        decoration: BoxDecoration(
                                          color: Colors.green
                                              .withValues(alpha: 0.15),
                                          borderRadius:
                                              BorderRadius.circular(12),
                                        ),
                                        child: Text(
                                          '${score.round()}% match',
                                          style: TextStyle(
                                            color: Colors.green.shade700,
                                            fontWeight: FontWeight.bold,
                                          ),
                                        ),
                                      );
                                    },
                                  ),
                                ],
                              ),
                              if (card.location.isNotEmpty || card.lastSeen > 0)
                                Padding(
                                  padding: const EdgeInsets.only(top: 4),
                                  child: Wrap(
                                    spacing: 12,
                                    runSpacing: 4,
                                    children: [
                                      if (card.location.isNotEmpty)
                                        Text(
                                          '📍 ${card.location}',
                                          style: Theme.of(context)
                                              .textTheme
                                              .bodySmall,
                                        ),
                                      if (card.lastSeen > 0)
                                        Text(
                                          _seenLabel(card.lastSeen),
                                          style: Theme.of(context)
                                              .textTheme
                                              .bodySmall,
                                        ),
                                    ],
                                  ),
                                ),
                              if (card.bio.isNotEmpty)
                                Padding(
                                  padding: const EdgeInsets.only(top: 8),
                                  child: Text(card.bio),
                                ),
                              if (card.interests.isNotEmpty)
                                Padding(
                                  padding: const EdgeInsets.only(top: 8),
                                  child: Wrap(
                                    spacing: 6,
                                    children: [
                                      for (final i in card.interests)
                                        Chip(
                                          label: Text('#$i'),
                                          labelStyle:
                                              const TextStyle(fontSize: 11),
                                          visualDensity: VisualDensity.compact,
                                        ),
                                    ],
                                  ),
                                ),
                              Padding(
                                padding: const EdgeInsets.only(top: 8),
                                child: Row(
                                  children: [
                                    TextButton.icon(
                                      icon: const Icon(Icons.block, size: 18),
                                      label: const Text('Block'),
                                      onPressed: () =>
                                          _blockCard(api, pubkey, card),
                                    ),
                                    const SizedBox(width: 8),
                                    TextButton.icon(
                                      icon: const Icon(
                                          Icons.warning_amber_rounded,
                                          size: 18),
                                      label: const Text('Report'),
                                      onPressed: () =>
                                          _reportCard(api, pubkey, card),
                                    ),
                                  ],
                                ),
                              ),
                            ],
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
            Padding(
              padding: const EdgeInsets.all(16),
              child: Row(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  IconButton.filledTonal(
                    iconSize: 32,
                    icon: const Icon(Icons.close),
                    tooltip: 'Pass',
                    onPressed: () async {
                      await api.pass(pubkey, card.pubkey);
                      setState(() => _cardIndex++);
                    },
                  ),
                  const SizedBox(width: 24),
                  IconButton.filled(
                    iconSize: 40,
                    icon: const Icon(Icons.favorite),
                    color: Colors.pink,
                    tooltip: 'Like',
                    onPressed: () async {
                      await api.like(pubkey, card.pubkey);
                      if (mounted) setState(() => _cardIndex++);
                      await _checkMatch(api, pubkey, card);
                    },
                  ),
                  const SizedBox(width: 24),
                  IconButton.filledTonal(
                    iconSize: 32,
                    icon: const Icon(Icons.star),
                    color: Colors.amber,
                    tooltip: 'Superlike',
                    onPressed: () async {
                      await api.superlike(pubkey, card.pubkey);
                      if (mounted) setState(() => _cardIndex++);
                      await _checkMatch(api, pubkey, card);
                    },
                  ),
                ],
              ),
            ),
          ],
        );
      },
    );
  }

  Widget _buildMatches(String pubkey) {
    return Consumer<DatingService>(
      builder: (context, api, _) => FutureBuilder(
        future: api.fetchMatches(pubkey),
        builder: (context, snapshot) {
          if (snapshot.connectionState != ConnectionState.done) {
            return const Center(child: CircularProgressIndicator());
          }
          final matches = snapshot.data ?? [];
          if (matches.isEmpty) {
            return const Center(child: Text('No matches yet'));
          }
          return ListView.builder(
            itemExtent: 72.0,
            itemCount: matches.length,
            itemBuilder: (context, index) {
              final m = matches[index];
              return ListTile(
                leading: m.images.isNotEmpty
                    ? CircleAvatar(
                        backgroundImage: ResizeImage.resizeIfNeeded(
                          128,
                          128,
                          NetworkImage(m.images.first),
                        ),
                      )
                    : const CircleAvatar(child: Icon(Icons.person)),
                title: Text(m.name.isEmpty ? firstChars(m.pubkey, 12) : m.name),
                subtitle: Text(firstChars(m.pubkey, 12)),
                trailing: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    FutureBuilder<double>(
                      future: _scoreFor(api, pubkey, m.pubkey),
                      builder: (context, snapshot) {
                        final score = snapshot.data ?? 0;
                        if (!snapshot.hasData || score <= 0) {
                          return const SizedBox.shrink();
                        }
                        return GestureDetector(
                          onTap: () =>
                              _showCompatibility(api, pubkey, m, score),
                          child: Container(
                            padding: const EdgeInsets.symmetric(
                                horizontal: 10, vertical: 4),
                            decoration: BoxDecoration(
                              color: Colors.green.withValues(alpha: 0.15),
                              borderRadius: BorderRadius.circular(12),
                            ),
                            child: Text(
                              '${score.round()}% match',
                              style: TextStyle(
                                color: Colors.green.shade700,
                                fontWeight: FontWeight.bold,
                              ),
                            ),
                          ),
                        );
                      },
                    ),
                    const SizedBox(width: 4),
                    TextButton(
                      onPressed: () => context.push('/inbox/${m.pubkey}'),
                      child: const Text('Message'),
                    ),
                    PopupMenuButton<String>(
                      padding: EdgeInsets.zero,
                      iconSize: 20,
                      tooltip: 'More',
                      onSelected: (value) {
                        if (value == 'unmatch') {
                          _confirmUnmatch(api, pubkey, m);
                        }
                      },
                      itemBuilder: (context) => const [
                        PopupMenuItem(
                          value: 'unmatch',
                          child: Text('Unmatch'),
                        ),
                      ],
                    ),
                  ],
                ),
                onTap: () => _showProfileDetail(api, pubkey, m),
              );
            },
          );
        },
      ),
    );
  }

  /// Match tile tap: fetch the fresh dating profile from the store
  /// (`dating_get_profile`) and show it as a detail bottom sheet.
  Future<void> _showProfileDetail(
    DatingService api,
    String pubkey,
    DatingCard match,
  ) async {
    final DatingCard profile;
    try {
      profile = await api.getProfile(match.pubkey);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Profile fetch error: $e')),
        );
      }
      return;
    }
    if (!mounted) return;
    final name =
        profile.name.isEmpty ? firstChars(profile.pubkey, 12) : profile.name;
    final score = await _scoreFor(api, pubkey, profile.pubkey);
    if (!mounted) return;
    await showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      builder: (context) => DraggableScrollableSheet(
        expand: false,
        initialChildSize: 0.6,
        maxChildSize: 0.9,
        builder: (context, scrollController) => ListView(
          controller: scrollController,
          padding: const EdgeInsets.fromLTRB(24, 0, 24, 32),
          children: [
            if (profile.images.isNotEmpty)
              ClipRRect(
                borderRadius: BorderRadius.circular(16),
                child: Image.network(
                  profile.images.first,
                  height: 200,
                  fit: BoxFit.cover,
                  errorBuilder: (_, __, ___) => Container(
                    height: 200,
                    color: Colors.grey[300],
                    child: const Icon(Icons.person, size: 64),
                  ),
                ),
              ),
            const SizedBox(height: 12),
            Text(
              profile.age > 0 ? '$name, ${profile.age}' : name,
              style: Theme.of(context).textTheme.headlineSmall,
            ),
            if (profile.location.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 4),
                child: Text('📍 ${profile.location}'),
              ),
            if (profile.bio.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(profile.bio),
              ),
            if (profile.interests.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Wrap(
                  spacing: 6,
                  children: [
                    for (final i in profile.interests)
                      Chip(
                        label: Text('#$i'),
                        labelStyle: const TextStyle(fontSize: 11),
                        visualDensity: VisualDensity.compact,
                      ),
                  ],
                ),
              ),
            const SizedBox(height: 16),
            if (score > 0)
              Center(
                child: Text(
                  '${score.round()}% compatibility',
                  style: Theme.of(context)
                      .textTheme
                      .titleMedium
                      ?.copyWith(color: Colors.green.shade700),
                ),
              ),
            const SizedBox(height: 12),
            Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                FilledButton.icon(
                  icon: const Icon(Icons.chat_bubble_outline),
                  label: const Text('Message'),
                  onPressed: () {
                    Navigator.pop(context);
                    context.push('/inbox/${profile.pubkey}');
                  },
                ),
                const SizedBox(width: 8),
                TextButton(
                  onPressed: () => Navigator.pop(context),
                  child: const Text('Close'),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildLikes(String pubkey) {
    return Consumer<DatingService>(
      builder: (context, api, _) => FutureBuilder(
        future: api.fetchLikes(pubkey),
        builder: (context, snapshot) {
          if (snapshot.connectionState != ConnectionState.done) {
            return const Center(child: CircularProgressIndicator());
          }
          final likes = snapshot.data ?? [];
          if (likes.isEmpty) {
            return const Center(child: Text('No likes received yet'));
          }
          return ListView.builder(
            itemExtent: 72.0,
            itemCount: likes.length,
            itemBuilder: (context, index) {
              final l = likes[index];
              return ListTile(
                leading: l.images.isNotEmpty
                    ? CircleAvatar(
                        backgroundImage: ResizeImage.resizeIfNeeded(
                          128,
                          128,
                          NetworkImage(l.images.first),
                        ),
                      )
                    : const CircleAvatar(child: Icon(Icons.person)),
                title: Text(l.name.isEmpty ? firstChars(l.pubkey, 12) : l.name),
                subtitle: Text('Liked you'),
                trailing: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    TextButton(
                      onPressed: () async {
                        await api.like(pubkey, l.pubkey);
                        if (context.mounted) {
                          ScaffoldMessenger.of(context).showSnackBar(
                            const SnackBar(
                                content: SelectableText('It\'s a match!')),
                          );
                        }
                      },
                      child: const Text('Match back'),
                    ),
                    IconButton(
                      icon: const Icon(Icons.person_remove, size: 20),
                      tooltip: 'Unlike',
                      visualDensity: VisualDensity.compact,
                      onPressed: () async {
                        await api.unlike(pubkey, l.pubkey);
                        if (context.mounted) {
                          ScaffoldMessenger.of(context).showSnackBar(
                            const SnackBar(content: Text('Removed like')),
                          );
                        }
                      },
                    ),
                  ],
                ),
              );
            },
          );
        },
      ),
    );
  }

  Future<double> _scoreFor(DatingService api, String pubkey, String target) =>
      _scores.putIfAbsent(target, () => api.calculateScore(pubkey, target));

  Future<void> _showCompatibility(
      DatingService api, String pubkey, DatingCard match, double score) async {
    final computed =
        score <= 0 ? await _scoreFor(api, pubkey, match.pubkey) : score;
    if (!mounted || computed <= 0) return;
    final name = match.name.isEmpty ? firstChars(match.pubkey, 12) : match.name;
    final String label;
    final String detail;
    if (computed >= 70) {
      label = 'Strong match';
      detail =
          'High overlap — shared values and lifestyle. Worth taking to a conversation.';
    } else if (computed >= 40) {
      label = 'Decent overlap';
      detail =
          'Some common ground, but the match is partial. A DM may still land well.';
    } else {
      label = 'Low overlap';
      detail =
          'Little shared ground on the dimensions we score. May still surprise — but temper expectations.';
    }
    await showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (context) => Padding(
        padding: const EdgeInsets.fromLTRB(24, 8, 24, 32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              '$name\n${computed.round()}%',
              textAlign: TextAlign.center,
              style: Theme.of(context).textTheme.headlineMedium,
            ),
            const SizedBox(height: 4),
            Text(
              label,
              style: Theme.of(context)
                  .textTheme
                  .titleMedium
                  ?.copyWith(color: Colors.green.shade700),
            ),
            const SizedBox(height: 12),
            Text(
              detail,
              textAlign: TextAlign.center,
              style: Theme.of(context).textTheme.bodyMedium,
            ),
            const SizedBox(height: 16),
            FilledButton(
              onPressed: () {
                Navigator.pop(context);
                context.push('/inbox/${match.pubkey}');
              },
              child: const Text('Send a Message'),
            ),
          ],
        ),
      ),
    );
  }

  String _seenLabel(int secs) {
    final diff = DateTime.now().millisecondsSinceEpoch ~/ 1000 - secs;
    if (diff < 60) return 'seen now';
    if (diff < 3600) return 'seen ${diff ~/ 60}m ago';
    if (diff < 86400) return 'seen ${diff ~/ 3600}h ago';
    return 'seen ${diff ~/ 86400}d ago';
  }

  Future<void> _blockCard(
      DatingService api, String pubkey, DatingCard card) async {
    await api.block(pubkey, card.pubkey);
    if (!mounted) return;
    setState(() => _cardIndex++);
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: const Text('Blocked'),
        action: SnackBarAction(
          label: 'Undo',
          onPressed: () async {
            await api.unblock(pubkey, card.pubkey);
            if (mounted) {
              setState(() =>
                  _cardIndex = (_cardIndex - 1).clamp(0, api.cards.length - 1));
            }
          },
        ),
      ),
    );
  }

  Future<void> _reportCard(
      DatingService api, String pubkey, DatingCard card) async {
    final reason = TextEditingController();
    final doIt = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Report profile'),
        content: TextField(
          controller: reason,
          decoration: const InputDecoration(labelText: 'Reason'),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Report'),
          ),
        ],
      ),
    );
    if (doIt != true || !mounted) return;
    await api.report(pubkey, card.pubkey,
        reason.text.trim().isEmpty ? 'reported' : reason.text.trim());
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Reported')),
      );
      setState(() => _cardIndex++);
    }
  }

  Future<void> _checkMatch(
      DatingService api, String pubkey, DatingCard liked) async {
    try {
      final likes = await api.fetchLikesForMatch(pubkey);
      if (!mounted) return;
      if (likes.any((l) => l.pubkey == liked.pubkey)) {
        setState(() => _matchCard = liked);
      }
    } catch (e) {
      debugPrint('dating match check: $e');
    }
  }

  Future<bool> _confirmUnmatch(
      DatingService api, String pubkey, DatingCard card) async {
    final name = card.name.isEmpty ? firstChars(card.pubkey, 12) : card.name;
    final doIt = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Unmatch?'),
        content: Text('Unmatch with $name? This cannot be undone.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Unmatch'),
          ),
        ],
      ),
    );
    if (doIt != true || !mounted) return false;
    final ok = await api.unmatch(pubkey, card.pubkey);
    if (!mounted) return false;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(ok ? 'Unmatched' : 'Failed to unmatch')),
    );
    return ok;
  }

  Widget _buildMatchOverlay(String pubkey) {
    final card = _matchCard!;
    final name = card.name.isEmpty ? firstChars(card.pubkey, 12) : card.name;
    return Positioned.fill(
      child: Container(
        color: Colors.black.withValues(alpha: 0.65),
        child: Center(
          child: Container(
            width: 340,
            margin: const EdgeInsets.symmetric(horizontal: 24),
            padding: const EdgeInsets.all(24),
            decoration: BoxDecoration(
              color: Theme.of(context).colorScheme.surface,
              borderRadius: BorderRadius.circular(24),
            ),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                const Text('💞', style: TextStyle(fontSize: 48)),
                const SizedBox(height: 8),
                Text(
                  'It\'s a Match!',
                  style: Theme.of(context).textTheme.headlineSmall,
                ),
                const SizedBox(height: 8),
                Text(
                  'You and $name liked each other.',
                  textAlign: TextAlign.center,
                ),
                const SizedBox(height: 4),
                Text(
                  firstChars(card.pubkey, 12),
                  style: Theme.of(context)
                      .textTheme
                      .bodySmall
                      ?.copyWith(color: Colors.grey),
                ),
                const SizedBox(height: 16),
                SizedBox(
                  width: double.infinity,
                  child: FilledButton(
                    onPressed: () {
                      final pk = card.pubkey;
                      setState(() => _matchCard = null);
                      context.push('/inbox/$pk');
                    },
                    child: const Text('💬 Send a Message'),
                  ),
                ),
                const SizedBox(height: 8),
                TextButton(
                  onPressed: () => setState(() => _matchCard = null),
                  child: const Text('Keep Browsing'),
                ),
                TextButton(
                  style: TextButton.styleFrom(
                    foregroundColor: Theme.of(context).colorScheme.error,
                  ),
                  onPressed: () async {
                    final api = context.read<DatingService>();
                    final ok = await _confirmUnmatch(api, pubkey, card);
                    if (!mounted) return;
                    if (ok) setState(() => _matchCard = null);
                  },
                  child: const Text('Unmatch'),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
