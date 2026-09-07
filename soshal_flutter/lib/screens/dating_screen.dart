import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/dating_service.dart';
import '../services/session_service.dart';
import '../theme/app_theme.dart';
import '../utils/dating_options.dart';
import '../utils/format.dart';
import '../widgets/blob_image.dart';
import '../widgets/empty_state.dart';

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
    _tabs = TabController(length: 4, vsync: this);
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
      if (pubkey == null) {
        setState(() => _loading = false);
        return;
      }
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
    final pubkey = context.select((SessionService s) => s.activePubkey);

    if (pubkey == null) {
      return Scaffold(
        appBar: AppBar(title: const Text('Dating')),
        body: const EmptyState(
          icon: Icons.favorite_border,
          title: 'Sign in to use Dating',
        ),
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
            icon: const Icon(Icons.settings),
            tooltip: 'Dating settings',
            onPressed: () => _showSettings(pubkey),
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
                            Tab(text: 'Secret Crush'),
                          ],
                        ),
                        Expanded(
                          child: TabBarView(
                            controller: _tabs,
                            children: [
                              _buildBrowse(pubkey),
                              _buildMatches(pubkey),
                              _buildLikes(pubkey),
                              _buildSecretCrush(pubkey),
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

  Widget _buildSecretCrush(String pubkey) {
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(Icons.lock_outline,
                  color: Theme.of(context).colorScheme.primary),
              const SizedBox(width: 8),
              Text(
                'Secret Crush (Up to 9 Picks)',
                style: Theme.of(context)
                    .textTheme
                    .titleMedium
                    ?.copyWith(fontWeight: FontWeight.bold),
              ),
            ],
          ),
          const SizedBox(height: 6),
          Text(
            'Select up to 9 friends or followers you are interested in. They will NEVER know unless they also add you to their Secret Crush list!',
            style: Theme.of(context).textTheme.bodySmall,
          ),
          const SizedBox(height: 16),
          Expanded(
            child: GridView.builder(
              gridDelegate: const SliverGridDelegateWithFixedCrossAxisCount(
                crossAxisCount: 3,
                crossAxisSpacing: 12,
                mainAxisSpacing: 12,
                childAspectRatio: 0.85,
              ),
              itemCount: 9,
              itemBuilder: (context, i) {
                return Card(
                  elevation: 1,
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(12),
                    side: BorderSide(
                      color: Theme.of(context).colorScheme.outlineVariant,
                      style: BorderStyle.solid,
                    ),
                  ),
                  child: InkWell(
                    borderRadius: BorderRadius.circular(12),
                    onTap: () {
                      ScaffoldMessenger.of(context).showSnackBar(
                        SnackBar(
                            content:
                                Text('Select friend for Crush Slot #${i + 1}')),
                      );
                    },
                    child: Column(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        CircleAvatar(
                          radius: 24,
                          backgroundColor:
                              Theme.of(context).colorScheme.primaryContainer,
                          child: Icon(Icons.add,
                              color: Theme.of(context).colorScheme.primary),
                        ),
                        const SizedBox(height: 8),
                        Text(
                          'Slot #${i + 1}',
                          style: const TextStyle(
                              fontSize: 12, fontWeight: FontWeight.w600),
                        ),
                        const SizedBox(height: 2),
                        const Text('Empty',
                            style: TextStyle(fontSize: 10, color: Colors.grey)),
                      ],
                    ),
                  ),
                );
              },
            ),
          ),
        ],
      ),
    );
  }

  Future<void> _showFilterDialog(String pubkey) async {
    final result = await showDialog<_FilterResult>(
        context: context, builder: (_) => const _FilterDialog());
    if (result == null || !mounted) return;
    final api = context.read<DatingService>();
    if (!result.ok) {
      await api.fetchProfiles(pubkey);
      return;
    }
    await api.filterProfiles(
      pubkey,
      minAge: result.minAge,
      maxAge: result.maxAge,
      radiusKm: result.radiusKm,
      heightMinCm: result.heightMinCm,
      heightMaxCm: result.heightMaxCm,
      bodyType: result.bodyType,
      smoking: result.smoking,
      drinking: result.drinking,
      relationshipIntent: result.intent,
      politics: result.politics,
      education: result.education,
      interests: result.interests,
    );
  }

  /// Dating settings sheet: edit profile or reset the "no" (pass) pile.
  Future<void> _showSettings(String pubkey) async {
    await showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (context) => SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            ListTile(
              leading: const Icon(Icons.badge_outlined),
              title: const Text('Edit profile'),
              subtitle: const Text('Update photos, interests, location'),
              trailing: const Icon(Icons.chevron_right),
              onTap: () {
                Navigator.pop(context);
                context.push('/dating/me');
              },
            ),
            ListTile(
              leading: const Icon(Icons.restart_alt),
              title: const Text('Reset profiles you passed'),
              subtitle: const Text('Swiped "no" profiles re-enter the deck'),
              trailing: const Icon(Icons.chevron_right),
              onTap: () {
                Navigator.pop(context);
                _resetPasses(pubkey);
              },
            ),
            const SizedBox(height: 8),
          ],
        ),
      ),
    );
  }

  Future<void> _resetPasses(String pubkey) async {
    final doIt = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Reset passed profiles?'),
        content: const Text(
          'Profiles you swiped "no" on will reappear in your deck. '
          'Likes and matches are untouched.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Reset'),
          ),
        ],
      ),
    );
    if (doIt != true || !mounted) return;
    final api = context.read<DatingService>();
    try {
      final n = await api.resetPasses(pubkey);
      await api.fetchProfiles(pubkey);
      if (!mounted) return;
      setState(() => _cardIndex = 0);
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: SelectableText(
            n > 0 ? 'Reset $n passed profiles' : 'No passed profiles to reset',
          ),
        ),
      );
    } catch (e, st) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Reset error: $e')),
        );
      }
      debugPrint('dating reset passes: $e\n$st');
    }
  }

  Widget _buildBrowse(String pubkey) {
    return Consumer<DatingService>(
      builder: (context, api, _) {
        if (api.cards.isEmpty) {
          return const EmptyState(
            icon: Icons.person_search_outlined,
            title: 'No profiles nearby yet',
          );
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
                          BlobImage(
                            source: card.images.first,
                            height: 220,
                            fit: BoxFit.cover,
                            errorBuilder: (_) => Container(
                              height: 220,
                              color: Theme.of(context)
                                  .colorScheme
                                  .surfaceContainerHighest,
                              child: const Icon(Icons.person, size: 80),
                            ),
                          )
                        else
                          Container(
                            height: 220,
                            color: Theme.of(context)
                                .colorScheme
                                .surfaceContainerHighest,
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
                      if (mounted) setState(() => _cardIndex++);
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
    return _MatchesTab(
      pubkey: pubkey,
      scoreFor: _scoreFor,
      onCompatibility: _showCompatibility,
      onUnmatch: _confirmUnmatch,
      onDetail: _showProfileDetail,
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
                child: BlobImage(
                  source: profile.images.first,
                  height: 200,
                  fit: BoxFit.cover,
                  errorBuilder: (_) => Container(
                    height: 200,
                    color:
                        Theme.of(context).colorScheme.surfaceContainerHighest,
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
            if (_profileChips(profile).isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Wrap(
                  spacing: 6,
                  runSpacing: 4,
                  children: [
                    for (final c in _profileChips(profile))
                      Chip(
                        label: Text(c),
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

  /// Attribute chips for the profile detail sheet.
  List<String> _profileChips(DatingCard p) {
    final chips = <String>[];
    void add(String label) {
      if (label.isNotEmpty) chips.add(label);
    }

    add(p.gender);
    add(p.seeking == 'All'
        ? 'seeking anyone'
        : p.seeking.isNotEmpty
            ? 'seeking ${p.seeking}'
            : '');
    if (p.height > 0) add('${p.height.round()} cm');
    add(p.bodyType);
    add(p.smoking.isNotEmpty ? 'smoking: ${p.smoking}' : '');
    add(p.drinking.isNotEmpty ? 'drinking: ${p.drinking}' : '');
    add(p.relationshipIntent);
    add(p.politics);
    add(p.education);
    add(p.ethnicity);
    if (p.language.isNotEmpty) add(p.language.join(', '));
    if (p.maxDistanceKm > 0) add('within ${p.maxDistanceKm.round()} km');
    return chips;
  }

  Widget _buildLikes(String pubkey) {
    return _LikesTab(pubkey: pubkey);
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
    final rel = relativeTime(secs);
    return rel == 'just now' ? 'seen now' : 'seen $rel';
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
    final result = await showDialog<_ReportCardResult>(
      context: context,
      builder: (_) => const _ReportDialog(),
    );
    if (result == null || !result.ok || !mounted) return;
    await api.report(pubkey, card.pubkey,
        result.reason.trim().isEmpty ? 'reported' : result.reason.trim());
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: SelectableText('Reported')),
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
      SnackBar(content: SelectableText(ok ? 'Unmatched' : 'Failed to unmatch')),
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

class _MatchesTab extends StatefulWidget {
  const _MatchesTab({
    required this.pubkey,
    required this.scoreFor,
    required this.onCompatibility,
    required this.onUnmatch,
    required this.onDetail,
  });

  final String pubkey;
  final Future<double> Function(DatingService api, String pubkey, String target)
      scoreFor;
  final void Function(
          DatingService api, String pubkey, DatingCard match, double score)
      onCompatibility;
  final Future<bool> Function(
      DatingService api, String pubkey, DatingCard match) onUnmatch;
  final Future<void> Function(
      DatingService api, String pubkey, DatingCard match) onDetail;

  @override
  State<_MatchesTab> createState() => _MatchesTabState();
}

class _MatchesTabState extends State<_MatchesTab> {
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      await context.read<DatingService>().fetchMatches(widget.pubkey);
    } catch (e) {
      debugPrint('matches: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  @override
  Widget build(BuildContext context) {
    final pubkey = widget.pubkey;
    return Consumer<DatingService>(
      builder: (context, api, _) {
        if (_loading) {
          return const Center(child: CircularProgressIndicator());
        }
        final matches = api.matches;
        if (matches.isEmpty) {
          return const EmptyState(
            icon: Icons.favorite_border,
            title: 'No matches yet',
          );
        }
        return ListView.builder(
          itemExtent: 72.0,
          itemCount: matches.length,
          itemBuilder: (context, index) {
            final m = matches[index];
            return ListTile(
              leading: m.images.isNotEmpty
                  ? ClipOval(
                      child: BlobImage(
                        source: m.images.first,
                        width: 48,
                        height: 48,
                        errorBuilder: (_) =>
                            const CircleAvatar(child: Icon(Icons.person)),
                      ),
                    )
                  : const CircleAvatar(child: Icon(Icons.person)),
              title: Text(m.name.isEmpty ? firstChars(m.pubkey, 12) : m.name),
              subtitle: Text(firstChars(m.pubkey, 12)),
              trailing: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  FutureBuilder<double>(
                    future: widget.scoreFor(api, pubkey, m.pubkey),
                    builder: (context, snapshot) {
                      final score = snapshot.data ?? 0;
                      if (!snapshot.hasData || score <= 0) {
                        return const SizedBox.shrink();
                      }
                      return GestureDetector(
                        onTap: () =>
                            widget.onCompatibility(api, pubkey, m, score),
                        child: Container(
                          padding: const EdgeInsets.symmetric(
                              horizontal: 10, vertical: 4),
                          decoration: BoxDecoration(
                            color: Theme.of(context)
                                    .extension<AppThemeExtension>()
                                    ?.glassGreen ??
                                Theme.of(context).colorScheme.primary,
                            borderRadius: BorderRadius.circular(12),
                          ),
                          child: Text(
                            '${score.round()}% match',
                            style: TextStyle(
                              color: Theme.of(context)
                                      .extension<AppThemeExtension>()
                                      ?.glassGreenText ??
                                  Theme.of(context).colorScheme.onPrimary,
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
                        widget.onUnmatch(api, pubkey, m);
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
              onTap: () => widget.onDetail(api, pubkey, m),
            );
          },
        );
      },
    );
  }
}

class _LikesTab extends StatefulWidget {
  const _LikesTab({required this.pubkey});

  final String pubkey;

  @override
  State<_LikesTab> createState() => _LikesTabState();
}

class _LikesTabState extends State<_LikesTab> {
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      await context.read<DatingService>().fetchLikes(widget.pubkey);
    } catch (e) {
      debugPrint('likes: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  @override
  Widget build(BuildContext context) {
    final pubkey = widget.pubkey;
    return Consumer<DatingService>(
      builder: (context, api, _) {
        if (_loading) {
          return const Center(child: CircularProgressIndicator());
        }
        final likes = api.likes;
        if (likes.isEmpty) {
          return const EmptyState(
            icon: Icons.favorite_outline,
            title: 'No likes received yet',
          );
        }
        return ListView.builder(
          itemExtent: 72.0,
          itemCount: likes.length,
          itemBuilder: (context, index) {
            final l = likes[index];
            return ListTile(
              leading: l.images.isNotEmpty
                  ? ClipOval(
                      child: BlobImage(
                        source: l.images.first,
                        width: 48,
                        height: 48,
                        errorBuilder: (_) =>
                            const CircleAvatar(child: Icon(Icons.person)),
                      ),
                    )
                  : const CircleAvatar(child: Icon(Icons.person)),
              title: Text(l.name.isEmpty ? firstChars(l.pubkey, 12) : l.name),
              subtitle: const Text('Liked you'),
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
                          SnackBar(content: SelectableText('Removed like')),
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
    );
  }
}

class _FilterResult {
  const _FilterResult({
    required this.ok,
    this.minAge = 0,
    this.maxAge = 0,
    this.radiusKm = 0,
    this.heightMinCm = 0,
    this.heightMaxCm = 0,
    this.bodyType = '',
    this.smoking = '',
    this.drinking = '',
    this.intent = '',
    this.politics = '',
    this.education = '',
    this.interests = const [],
  });

  final bool ok;
  final int minAge;
  final int maxAge;
  final int radiusKm;
  final int heightMinCm;
  final int heightMaxCm;
  final String bodyType;
  final String smoking;
  final String drinking;
  final String intent;
  final String politics;
  final String education;
  final List<String> interests;
}

/// Filter dialog. Owns its TextEditingControllers so they stay alive through
/// the dialog's exit transition and are disposed only after the route is
/// fully removed (disposing them in a `finally` after `showDialog` completes
/// crashed on every close — the closing animation still rebuilds the
/// TextFields against disposed controllers).
class _FilterDialog extends StatefulWidget {
  const _FilterDialog();

  @override
  State<_FilterDialog> createState() => _FilterDialogState();
}

class _FilterDialogState extends State<_FilterDialog> {
  final _minAge = TextEditingController();
  final _maxAge = TextEditingController();
  final _interests = TextEditingController();
  final _minHeight = TextEditingController();
  final _maxHeight = TextEditingController();
  int _radiusKm = 0;
  int _heightMinCm = 0;
  int _heightMaxCm = 0;
  String _bodyType = '';
  String _smoking = '';
  String _drinking = '';
  String _intent = '';
  String _politics = '';
  String _education = '';

  @override
  void dispose() {
    _minAge.dispose();
    _maxAge.dispose();
    _interests.dispose();
    _minHeight.dispose();
    _maxHeight.dispose();
    super.dispose();
  }

  void _pop(bool ok) {
    Navigator.of(context).pop(_FilterResult(
      ok: ok,
      minAge: int.tryParse(_minAge.text.trim()) ?? 0,
      maxAge: int.tryParse(_maxAge.text.trim()) ?? 0,
      radiusKm: _radiusKm,
      heightMinCm: _heightMinCm,
      heightMaxCm: _heightMaxCm,
      bodyType: _bodyType,
      smoking: _smoking,
      drinking: _drinking,
      intent: _intent,
      politics: _politics,
      education: _education,
      interests: _interests.text
          .split(',')
          .map((e) => e.trim())
          .where((e) => e.isNotEmpty)
          .toList(),
    ));
  }

  Widget _dropdown(
    String label,
    String value,
    List<String> options,
    ValueChanged<String> onChanged,
  ) {
    return DropdownButtonFormField<String>(
      initialValue: value,
      decoration: InputDecoration(labelText: label),
      items: [
        for (final o in options)
          DropdownMenuItem(value: o, child: Text(o.isEmpty ? 'Any' : o)),
      ],
      onChanged: (v) => onChanged(v ?? ''),
    );
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Filter profiles'),
      content: SizedBox(
        width: 320,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                controller: _minAge,
                keyboardType: TextInputType.number,
                decoration: const InputDecoration(labelText: 'Min age'),
              ),
              TextField(
                controller: _maxAge,
                keyboardType: TextInputType.number,
                decoration: const InputDecoration(labelText: 'Max age'),
              ),
              const SizedBox(height: 8),
              ListTile(
                contentPadding: EdgeInsets.zero,
                title: const Text('Radius'),
                subtitle: Text(_radiusKm <= 0 ? 'Unlimited' : '$_radiusKm km'),
                trailing: SizedBox(
                  width: 120,
                  child: Slider(
                    min: 0,
                    max: 500,
                    divisions: 10,
                    value: _radiusKm.toDouble(),
                    label: _radiusKm <= 0 ? 'Unlimited' : '$_radiusKm km',
                    onChanged: (v) => setState(() => _radiusKm = v.round()),
                  ),
                ),
              ),
              Row(
                children: [
                  Expanded(
                    child: TextField(
                      controller: _minHeight,
                      keyboardType: TextInputType.number,
                      decoration:
                          const InputDecoration(labelText: 'Min height cm'),
                      onChanged: (v) =>
                          _heightMinCm = int.tryParse(v.trim()) ?? 0,
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: TextField(
                      controller: _maxHeight,
                      keyboardType: TextInputType.number,
                      decoration:
                          const InputDecoration(labelText: 'Max height cm'),
                      onChanged: (v) =>
                          _heightMaxCm = int.tryParse(v.trim()) ?? 0,
                    ),
                  ),
                ],
              ),
              _dropdown('Body type', _bodyType, kBodyTypeOptions,
                  (v) => setState(() => _bodyType = v)),
              _dropdown('Smoking', _smoking, kSmokingOptions,
                  (v) => setState(() => _smoking = v)),
              _dropdown('Drinking', _drinking, kDrinkingOptions,
                  (v) => setState(() => _drinking = v)),
              _dropdown('Relationship intent', _intent, kIntentOptions,
                  (v) => setState(() => _intent = v)),
              _dropdown('Politics', _politics, kPoliticsOptions,
                  (v) => setState(() => _politics = v)),
              _dropdown('Education', _education, kEducationOptions,
                  (v) => setState(() => _education = v)),
              TextField(
                controller: _interests,
                decoration: const InputDecoration(
                  labelText: 'Interests',
                  hintText: 'comma separated',
                ),
              ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => _pop(false),
          child: const Text('Clear'),
        ),
        FilledButton(
          onPressed: () => _pop(true),
          child: const Text('Apply'),
        ),
      ],
    );
  }
}

class _ReportCardResult {
  const _ReportCardResult({required this.ok, required this.reason});

  final bool ok;
  final String reason;
}

class _ReportDialog extends StatefulWidget {
  const _ReportDialog();

  @override
  State<_ReportDialog> createState() => _ReportDialogState();
}

class _ReportDialogState extends State<_ReportDialog> {
  final _reason = TextEditingController();

  @override
  void dispose() {
    _reason.dispose();
    super.dispose();
  }

  void _pop(bool ok) {
    Navigator.of(context).pop(_ReportCardResult(
      ok: ok,
      reason: _reason.text.trim(),
    ));
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Report profile'),
      content: TextField(
        controller: _reason,
        decoration: const InputDecoration(labelText: 'Reason'),
      ),
      actions: [
        TextButton(
          onPressed: () => _pop(false),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: () => _pop(true),
          child: const Text('Report'),
        ),
      ],
    );
  }
}
