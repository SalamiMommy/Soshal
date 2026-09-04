import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/moderation_service.dart';
import '../services/session_service.dart';
import '../widgets/settings_scaffold.dart';

/// Moderation Settings
/// Muted users, word filters, and a content-filter test.
class ModerationScreen extends StatefulWidget {
  const ModerationScreen({super.key});

  @override
  State<ModerationScreen> createState() => _ModerationScreenState();
}

class _ModerationScreenState extends State<ModerationScreen> {
  String? _pubkey;
  final _testContent = TextEditingController();
  bool? _filtered;
  HybridModerationResult? _hybridResult;
  bool _forceDeepScan = false;
  List<dynamic> _reports = [];
  bool _reportsLoading = false;
  final _checkTarget = TextEditingController();
  ({bool muted, bool blocked, bool restricted})? _restrictionResult;
  String? _juryCaseJson;
  String? _juryResult;

  @override
  void dispose() {
    _testContent.dispose();
    _checkTarget.dispose();
    super.dispose();
  }

  @override
  void initState() {
    super.initState();
    _pubkey = context.read<SessionService>().activePubkey;
    context.read<ModerationService>().load(_pubkey ?? '');
    _loadReports();
  }

  Future<void> _loadReports() async {
    final target = _pubkey;
    if (target == null) return;
    if (!mounted) return;
    setState(() => _reportsLoading = true);
    final reports = await context.read<ModerationService>().listReports(target);
    if (!mounted) return;
    setState(() {
      _reports = reports;
      _reportsLoading = false;
    });
  }

  Future<void> _deleteReport(String reportId) async {
    final ok = await context.read<ModerationService>().deleteReport(reportId);
    if (ok) await _loadReports();
  }

  Future<void> _unblock(String targetPubkey) async {
    final me = _pubkey;
    if (me == null) return;
    await context.read<ModerationService>().unblock(me, targetPubkey);
  }

  Future<void> _checkRestriction() async {
    final me = _pubkey;
    final target = _checkTarget.text.trim();
    if (me == null || target.isEmpty) return;
    final mod = context.read<ModerationService>();
    if (!mounted) return;
    setState(() {
      _restrictionResult = (
        muted: mod.isMuted(target),
        blocked: mod.isBlocked(target),
        restricted: mod.isRestricted(me, target),
      );
    });
  }

  Future<void> _createJuryCase() async {
    final caseId = TextEditingController(
        text: DateTime.now().millisecondsSinceEpoch.toString());
    final target = TextEditingController();
    final reason = TextEditingController();
    final threshold = TextEditingController(text: '2');
    final total = TextEditingController(text: '3');
    final group = TextEditingController();
    try {
      final ok = await showDialog<bool>(
        context: context,
        builder: (context) => AlertDialog(
          title: const Text('Create jury case'),
          content: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextField(
                  controller: caseId,
                  decoration: const InputDecoration(labelText: 'Case id'),
                ),
                TextField(
                  controller: target,
                  decoration: const InputDecoration(labelText: 'Target pubkey'),
                ),
                TextField(
                  controller: reason,
                  decoration: const InputDecoration(labelText: 'Reason'),
                ),
                TextField(
                  controller: threshold,
                  keyboardType: TextInputType.number,
                  decoration: const InputDecoration(labelText: 'Threshold'),
                ),
                TextField(
                  controller: total,
                  keyboardType: TextInputType.number,
                  decoration: const InputDecoration(labelText: 'Total jurors'),
                ),
                TextField(
                  controller: group,
                  decoration: const InputDecoration(labelText: 'Group pubkey'),
                ),
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: const Text('Create'),
            ),
          ],
        ),
      );
      if (ok != true || !mounted) return;
      try {
        final json = await context.read<ModerationService>().createJuryCase(
              caseId: caseId.text.trim(),
              targetPubkey: target.text.trim(),
              reason: reason.text.trim(),
              threshold: int.tryParse(threshold.text.trim()) ?? 2,
              totalJurors: int.tryParse(total.text.trim()) ?? 3,
              groupPubkey: group.text.trim(),
            );
        if (!mounted) return;
        setState(() {
          _juryCaseJson = json;
          _juryResult = null;
        });
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(content: SelectableText('Jury case failed: $e')));
        }
      }
    } finally {
      caseId.dispose();
      target.dispose();
      reason.dispose();
      threshold.dispose();
      total.dispose();
      group.dispose();
    }
  }

  Future<void> _addFilter() async {
    final controller = TextEditingController();
    try {
      final value = await showDialog<String>(
        context: context,
        builder: (context) => AlertDialog(
          title: const Text('Add word filter'),
          content: TextField(
            controller: controller,
            decoration: const InputDecoration(hintText: 'word or phrase'),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, controller.text.trim()),
              child: const Text('Add'),
            ),
          ],
        ),
      );
      if (value == null || value.isEmpty) return;
      if (!mounted) return;
      final api = context.read<ModerationService>();
      await api.setWordFilters([...api.wordFilters, value]);
    } finally {
      controller.dispose();
    }
  }

  Future<void> _removeFilter(String filter) async {
    final api = context.read<ModerationService>();
    await api
        .setWordFilters(api.wordFilters.where((f) => f != filter).toList());
  }

  @override
  Widget build(BuildContext context) {
    final api = context.read<ModerationService>();
    return SettingsScaffold(
      title: 'Moderation',
      padding: const EdgeInsets.all(16),
      children: [
        Text('Muted users', style: Theme.of(context).textTheme.titleMedium),
        Consumer<ModerationService>(
          builder: (context, mod, _) {
            if (mod.muted.isEmpty) {
              return const Padding(
                padding: EdgeInsets.symmetric(vertical: 8),
                child: Text('No muted users'),
              );
            }
            return Column(
              children: [
                for (final muted in mod.muted)
                  ListTile(
                    dense: true,
                    leading: const Icon(Icons.volume_off),
                    title: Text(muted,
                        maxLines: 1, overflow: TextOverflow.ellipsis),
                    trailing: IconButton(
                      icon: const Icon(Icons.undo),
                      tooltip: 'Unmute',
                      onPressed: () async {
                        final pubkey = _pubkey;
                        if (pubkey == null) return;
                        await mod.unmute(pubkey, muted);
                      },
                    ),
                  ),
              ],
            );
          },
        ),
        const Divider(height: 32),
        Text('Blocked users', style: Theme.of(context).textTheme.titleMedium),
        Consumer<ModerationService>(
          builder: (context, mod, _) {
            if (mod.blocked.isEmpty) {
              return const Padding(
                padding: EdgeInsets.symmetric(vertical: 8),
                child: Text('No blocked users'),
              );
            }
            return Column(
              children: [
                for (final blocked in mod.blocked)
                  ListTile(
                    dense: true,
                    leading: const Icon(Icons.block),
                    title: Text(blocked,
                        maxLines: 1, overflow: TextOverflow.ellipsis),
                    trailing: IconButton(
                      icon: const Icon(Icons.undo),
                      tooltip: 'Unblock',
                      onPressed: () => _unblock(blocked),
                    ),
                  ),
              ],
            );
          },
        ),
        const Divider(height: 32),
        Text('Reports', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 8),
        if (_reportsLoading)
          const Padding(
            padding: EdgeInsets.symmetric(vertical: 16),
            child: Center(child: CircularProgressIndicator()),
          )
        else if (_reports.isEmpty)
          const Padding(
            padding: EdgeInsets.symmetric(vertical: 8),
            child: Text('No reports'),
          )
        else
          for (final report in _reports)
            ListTile(
              dense: true,
              leading: const Icon(Icons.flag_outlined),
              title: Text(
                report['pubkey'] as String? ?? '',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
              subtitle: Text(
                report['reason'] as String? ?? '',
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
              ),
              trailing: IconButton(
                icon: const Icon(Icons.delete_outline),
                tooltip: 'Delete report',
                onPressed: () => _deleteReport(report['id'] as String? ?? ''),
              ),
            ),
        const Divider(height: 32),
        Text('Restriction check',
            style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 8),
        TextField(
          controller: _checkTarget,
          decoration: const InputDecoration(
            hintText: 'Pubkey to check',
            border: OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 8),
        FilledButton.icon(
          icon: const Icon(Icons.verified_user_outlined),
          label: const Text('Check'),
          onPressed: _checkRestriction,
        ),
        if (_restrictionResult != null)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(
              'muted: ${_restrictionResult!.muted} · '
              'blocked: ${_restrictionResult!.blocked} · '
              'restricted: ${_restrictionResult!.restricted}',
              style: const TextStyle(fontWeight: FontWeight.bold),
            ),
          ),
        const Divider(height: 32),
        Row(
          children: [
            Text('Community jury',
                style: Theme.of(context).textTheme.titleMedium),
            const Spacer(),
            IconButton(
              icon: const Icon(Icons.gavel),
              tooltip: 'Create jury case',
              onPressed: _createJuryCase,
            ),
          ],
        ),
        if (_juryCaseJson != null) ...[
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 8),
            child: Text(
              _juryCaseJson!,
              maxLines: 3,
              overflow: TextOverflow.ellipsis,
              style: const TextStyle(fontSize: 12),
            ),
          ),
          Text(
            'FROST jury voting unavailable: threshold signing on roadmap',
            style: const TextStyle(fontSize: 12),
          ),
        ],
        if (_juryResult != null)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(
              _juryResult!,
              maxLines: 4,
              overflow: TextOverflow.ellipsis,
              style: const TextStyle(fontSize: 12),
            ),
          ),
        const Divider(height: 32),
        Row(
          children: [
            Text('Word filters',
                style: Theme.of(context).textTheme.titleMedium),
            const Spacer(),
            IconButton(
              icon: const Icon(Icons.add),
              tooltip: 'Add filter',
              onPressed: _addFilter,
            ),
          ],
        ),
        Consumer<ModerationService>(
          builder: (context, mod, _) {
            if (mod.wordFilters.isEmpty) {
              return const Padding(
                padding: EdgeInsets.symmetric(vertical: 8),
                child: Text('No word filters'),
              );
            }
            return Wrap(
              spacing: 8,
              children: [
                for (final f in mod.wordFilters)
                  Chip(
                    label: Text(f),
                    onDeleted: () => _removeFilter(f),
                  ),
              ],
            );
          },
        ),
        const Divider(height: 32),
        Row(
          mainAxisAlignment: MainAxisAlignment.spaceBetween,
          children: [
            Text('2-Tier Hybrid AI Scanner',
                style: Theme.of(context).textTheme.titleMedium),
            Row(
              children: [
                const Text('Deep Scan (RoBERTa):',
                    style: TextStyle(fontSize: 12)),
                Switch(
                  value: _forceDeepScan,
                  onChanged: (v) => setState(() => _forceDeepScan = v),
                ),
              ],
            ),
          ],
        ),
        Text(
          'Tier 1 (N-Gram Subword) + Tier 2 (heuristic embeddings & perceptual image hash; '
          'real ML model on roadmap)',
          style: Theme.of(context).textTheme.bodySmall,
        ),
        const SizedBox(height: 8),
        TextField(
          controller: _testContent,
          maxLines: 3,
          decoration: const InputDecoration(
            hintText: 'Paste content to scan through 2-Tier Hybrid AI',
            border: OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 8),
        FilledButton.icon(
          icon: const Icon(Icons.psychology_outlined),
          label: const Text('Scan with 2-Tier Hybrid AI'),
          onPressed: () async {
            final pubkey = _pubkey ?? '';
            final text = _testContent.text;
            try {
              final filtered = await api.shouldFilter(text, pubkey);
              final hybrid = await api.hybridClassifyText(
                text,
                forceDeepScan: _forceDeepScan,
              );
              if (mounted) {
                setState(() {
                  _filtered = filtered;
                  _hybridResult = hybrid;
                });
              }
            } catch (e) {
              if (!context.mounted) return;
              ScaffoldMessenger.of(context).showSnackBar(
                SnackBar(content: Text('Scan failed: $e')),
              );
            }
          },
        ),
        if (_filtered != null && _hybridResult != null) ...[
          const SizedBox(height: 12),
          Card(
            elevation: 2,
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Icon(
                        _hybridResult!.isFlagged || _filtered!
                            ? Icons.warning_amber_rounded
                            : Icons.check_circle_outline,
                        color: _hybridResult!.isFlagged || _filtered!
                            ? Colors.red
                            : Colors.green,
                      ),
                      const SizedBox(width: 8),
                      Text(
                        _hybridResult!.isFlagged
                            ? 'Flagged (${_hybridResult!.primaryCategory?.toUpperCase() ?? "HAZARD"})'
                            : (_filtered!
                                ? 'Filtered by word/block list'
                                : 'Safe — Content Passes 2-Tier AI'),
                        style: TextStyle(
                          fontWeight: FontWeight.bold,
                          color: _hybridResult!.isFlagged || _filtered!
                              ? Colors.red
                              : Colors.green,
                        ),
                      ),
                      const Spacer(),
                      Container(
                        padding: const EdgeInsets.symmetric(
                            horizontal: 8, vertical: 2),
                        decoration: BoxDecoration(
                          color: Theme.of(context)
                              .colorScheme
                              .surfaceContainerHighest,
                          borderRadius: BorderRadius.circular(12),
                          border: Border.all(
                            color: Theme.of(context).colorScheme.primary,
                          ),
                        ),
                        child: Text(
                          _hybridResult!.tierEvaluated == 'Tier2Deep'
                              ? 'Tier 2: RoBERTa'
                              : 'Tier 1: Fast N-Gram',
                          style: TextStyle(
                            fontSize: 11,
                            fontWeight: FontWeight.bold,
                            color: Theme.of(context).colorScheme.primary,
                          ),
                        ),
                      ),
                    ],
                  ),
                  const Divider(height: 16),
                  _buildScoreRow(
                    context,
                    'Spam & Scams',
                    _hybridResult!.tier1Result.scores.spam,
                    Colors.orange,
                  ),
                  _buildScoreRow(
                    context,
                    'CSAM Zero-Tolerance',
                    _hybridResult!.tier1Result.scores.csam,
                    Colors.red.shade900,
                  ),
                  _buildScoreRow(
                    context,
                    'Gore & Violence',
                    _hybridResult!.tier1Result.scores.gore,
                    Colors.deepOrange,
                  ),
                  _buildScoreRow(
                    context,
                    'Bigotry & Hate Speech',
                    _hybridResult!.tier1Result.scores.bigotry,
                    Colors.purple,
                  ),
                  _buildScoreRow(
                    context,
                    'Targeted Harassment',
                    _hybridResult!.tier1Result.scores.harassment,
                    Colors.indigo,
                  ),
                  if (_hybridResult!.tier2RobertaResult != null) ...[
                    const Divider(height: 16),
                    Text(
                      'RoBERTa Transformer Semantic Scores:',
                      style: Theme.of(context).textTheme.labelMedium,
                    ),
                    const SizedBox(height: 4),
                    _buildScoreRow(
                      context,
                      'RoBERTa Toxicity',
                      _hybridResult!.tier2RobertaResult!.scores.toxic,
                      Colors.redAccent,
                    ),
                    _buildScoreRow(
                      context,
                      'RoBERTa Threat',
                      _hybridResult!.tier2RobertaResult!.scores.threat,
                      Colors.deepPurple,
                    ),
                    _buildScoreRow(
                      context,
                      'RoBERTa Identity Hate',
                      _hybridResult!.tier2RobertaResult!.scores.identityHate,
                      Colors.purpleAccent,
                    ),
                  ],
                  if (_hybridResult!.tier1Result.evasionScore > 0.05) ...[
                    const SizedBox(height: 6),
                    _buildScoreRow(
                      context,
                      'Obfuscation / Evasion Score',
                      _hybridResult!.tier1Result.evasionScore,
                      Colors.amber.shade800,
                    ),
                  ],
                  if (_hybridResult!.detectedReasons.isNotEmpty) ...[
                    const SizedBox(height: 8),
                    Text(
                      'Detection Signals:',
                      style: Theme.of(context).textTheme.labelMedium,
                    ),
                    const SizedBox(height: 4),
                    Wrap(
                      spacing: 6,
                      runSpacing: 4,
                      children: [
                        for (final r in _hybridResult!.detectedReasons)
                          Chip(
                            label:
                                Text(r, style: const TextStyle(fontSize: 11)),
                            padding: EdgeInsets.zero,
                            visualDensity: VisualDensity.compact,
                          ),
                      ],
                    ),
                  ],
                ],
              ),
            ),
          ),
        ],
      ],
    );
  }

  Widget _buildScoreRow(
    BuildContext context,
    String label,
    double score,
    Color color,
  ) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Text(label, style: const TextStyle(fontSize: 12)),
              Text(
                '${(score * 100).toStringAsFixed(1)}%',
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: FontWeight.bold,
                  color: score > 0.5 ? color : Colors.grey,
                ),
              ),
            ],
          ),
          const SizedBox(height: 2),
          LinearProgressIndicator(
            value: score.clamp(0.0, 1.0),
            backgroundColor:
                Theme.of(context).colorScheme.surfaceContainerHighest,
            valueColor: AlwaysStoppedAnimation<Color>(
              score > 0.5 ? color : Theme.of(context).colorScheme.outline,
            ),
            minHeight: 4,
          ),
        ],
      ),
    );
  }
}
