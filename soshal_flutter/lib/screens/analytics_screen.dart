import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/analytics_service.dart';
import '../widgets/error_state_text.dart';

/// Analytics: local SLM stats, post classification and embeddings.
class AnalyticsScreen extends StatefulWidget {
  /// Analytics screen.
  const AnalyticsScreen({super.key});

  @override
  State<AnalyticsScreen> createState() => _AnalyticsScreenState();
}

class _AnalyticsScreenState extends State<AnalyticsScreen> {
  final TextEditingController _classifyText = TextEditingController();
  final TextEditingController _embedText = TextEditingController();
  String? _stats;
  String? _classification;
  String? _embeddingSummary;
  String? _error;

  @override
  void dispose() {
    _classifyText.dispose();
    _embedText.dispose();
    super.dispose();
  }

  Future<void> _loadStats() async {
    setState(() => _error = null);
    try {
      final out = await context.read<AnalyticsService>().computeStats();
      if (mounted) setState(() => _stats = out);
    } catch (e) {
      debugPrint('analytics stats: $e');
      if (mounted) setState(() => _error = e.toString());
    }
  }

  Future<void> _runClassify() async {
    final text = _classifyText.text.trim();
    if (text.isEmpty) return;
    setState(() => _error = null);
    try {
      final out = await context.read<AnalyticsService>().classifyPost(text);
      if (mounted) {
        setState(() {
          _classification = _prettyJson(out);
        });
      }
    } catch (e) {
      debugPrint('analytics classify: $e');
      if (mounted) setState(() => _error = e.toString());
    }
  }

  Future<void> _runEmbedding() async {
    final text = _embedText.text.trim();
    if (text.isEmpty) return;
    setState(() => _error = null);
    try {
      final out =
          await context.read<AnalyticsService>().generateEmbedding(text);
      if (mounted) setState(() => _embeddingSummary = _summarizeEmbedding(out));
    } catch (e) {
      debugPrint('analytics embedding: $e');
      if (mounted) setState(() => _error = e.toString());
    }
  }

  String _prettyJson(String raw) {
    try {
      final decoded = jsonDecode(raw);
      return const JsonEncoder.withIndent('  ').convert(decoded);
    } catch (_) {
      return raw;
    }
  }

  String _summarizeEmbedding(String raw) {
    try {
      final decoded = jsonDecode(raw) as Map<String, dynamic>;
      final vector = (decoded['vector'] as List<dynamic>? ?? [])
          .map((v) => v as num)
          .toList();
      final dimension =
          (decoded['dimension'] as num?)?.toInt() ?? vector.length;
      final first = vector.take(8).map((v) => v.toStringAsFixed(3)).join(', ');
      return 'dimension: $dimension\nfirst values: [$first${vector.length > 8 ? ', ...' : ''}]';
    } catch (_) {
      return raw;
    }
  }

  Widget _section(String title, Widget child) {
    return Card(
      margin: const EdgeInsets.all(16),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(title, style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 12),
            child,
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Analytics')),
      body: ListView(
        children: [
          _section(
            'Post Analytics',
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Text(
                  'Engagement statistics computed locally in Rust.',
                  style: TextStyle(color: Colors.grey),
                ),
                const SizedBox(height: 12),
                FilledButton.icon(
                  onPressed: _loadStats,
                  icon: const Icon(Icons.query_stats),
                  label: const Text('Run Analytics'),
                ),
                if (_stats != null) ...[
                  const SizedBox(height: 12),
                  SelectableText(
                    _prettyJson(_stats!),
                    style:
                        const TextStyle(fontFamily: 'monospace', fontSize: 12),
                  ),
                ],
              ],
            ),
          ),
          _section(
            'Classify Post',
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                TextField(
                  controller: _classifyText,
                  maxLines: 3,
                  decoration: const InputDecoration(
                    hintText: 'Paste post text to classify…',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 12),
                FilledButton(
                  onPressed: _runClassify,
                  child: const Text('Classify'),
                ),
                if (_classification != null) ...[
                  const SizedBox(height: 12),
                  SelectableText(
                    _classification!,
                    style:
                        const TextStyle(fontFamily: 'monospace', fontSize: 12),
                  ),
                ],
              ],
            ),
          ),
          _section(
            'Generate Embedding',
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                TextField(
                  controller: _embedText,
                  maxLines: 3,
                  decoration: const InputDecoration(
                    hintText: 'Text to embed (local SLM, 384 dims)…',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 12),
                FilledButton(
                  onPressed: _runEmbedding,
                  child: const Text('Embed'),
                ),
                if (_embeddingSummary != null) ...[
                  const SizedBox(height: 12),
                  SelectableText(
                    _embeddingSummary!,
                    style:
                        const TextStyle(fontFamily: 'monospace', fontSize: 12),
                  ),
                ],
              ],
            ),
          ),
          if (_error != null)
            Padding(
              padding: const EdgeInsets.all(16),
              child: ErrorStateText('Error: $_error'),
            ),
        ],
      ),
    );
  }
}
