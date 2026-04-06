import { useEffect, useMemo, useState } from 'react';
import {
  ArrowUpRight,
  CheckCircle2,
  CircleDashed,
  Cpu,
  Download,
  FileText,
  LoaderCircle,
  Sparkles,
  TimerReset,
} from 'lucide-react';
import { cn } from '../lib/utils';

interface ProcessItemProps {
  label: string;
  state: 'done' | 'live' | 'queued';
}

function ProcessItem({ label, state }: ProcessItemProps) {
  return (
    <div
      className={cn(
        'flex flex-col items-start gap-1 px-3 py-2.5 rounded-[0.9rem] border transition-all w-full min-w-0',
        state === 'done' && 'bg-[rgba(60,217,159,0.08)] border-[rgba(60,217,159,0.18)]',
        state === 'live' && 'bg-[rgba(104,166,255,0.08)] border-[rgba(104,166,255,0.18)]',
        state === 'queued' && 'bg-[rgba(255,255,255,0.03)] border-[rgba(255,255,255,0.05)]'
      )}
    >
      <span className="text-xs text-[var(--color-text-secondary)]">{label}</span>
      <span
        className={cn(
          'text-[0.62rem] font-bold uppercase tracking-wider px-2 py-1 rounded-full border whitespace-nowrap',
          state === 'done' && 'text-[var(--color-accent-green)] bg-[rgba(60,217,159,0.12)] border-[rgba(60,217,159,0.28)]',
          state === 'live' && 'text-[var(--color-accent-blue)] bg-[rgba(104,166,255,0.12)] border-[rgba(104,166,255,0.28)]',
          state === 'queued' && 'text-[var(--color-text-tertiary)] bg-[rgba(255,255,255,0.04)] border-[rgba(255,255,255,0.12)]'
        )}
      >
        {state === 'done' ? 'Done' : state === 'live' ? 'Live' : 'Queued'}
      </span>
    </div>
  );
}

export default function AppMockup() {
  const [progress, setProgress] = useState(12);
  const [livePage, setLivePage] = useState(9);
  const [typingText, setTypingText] = useState('');
  const [activeSection, setActiveSection] = useState<'pipeline' | 'quality' | 'export'>('pipeline');
  const fullText = '- Revenue grew 18% year-over-year across enterprise accounts.';

  const status = useMemo(
    () => ({
      input: {
        file: 'Q4_Enterprise_Review.pdf',
        pages: 18,
        size: '1.8 MB',
      },
      provider: 'OpenRouter · GPT-4.1',
      template: 'General document parsing',
    }),
    []
  );

  const keyMetrics = useMemo(
    () => [
      { title: 'Conversion', value: '18 / 18', note: 'pages', tone: 'blue' },
      { title: 'Extraction', value: '96.7%', note: 'LLM confidence', tone: 'purple' },
      { title: 'Output', value: '+3.2k', note: 'markdown chars', tone: 'gold' },
      { title: 'Images', value: '18', note: 'saved assets', tone: 'cyan' },
    ],
    []
  );

  const fileRows = useMemo(
    () => [
      {
        page: '01',
        section: 'Executive summary',
        status: 'done',
      },
      {
        page: '02',
        section: 'Revenue overview',
        status: 'done',
      },
      {
        page: `${livePage.toString().padStart(2, '0')}`,
        section: 'AI extraction in progress',
        status: 'live',
      },
      {
        page: `${livePage + 1}`,
        section: 'Pending batch',
        status: 'queued',
      },
    ],
    [livePage]
  );

  useEffect(() => {
    const handle = setInterval(() => {
      setLivePage((current) => (current >= status.input.pages ? 9 : current + 1));
    }, 1800);

    return () => clearInterval(handle);
  }, [status.input.pages]);

  const sectionTabs = useMemo(
    () => [
      { key: 'pipeline', label: 'Pipeline', icon: Cpu },
      { key: 'quality', label: 'Quality', icon: Sparkles },
      { key: 'export', label: 'Export', icon: FileText },
    ] as const,
    []
  );

  useEffect(() => {
    const progressInterval = setInterval(() => {
      setProgress((prev) => {
        if (prev >= 18) return 12;
        return prev + 1;
      });
    }, 2200);

    return () => clearInterval(progressInterval);
  }, []);

  useEffect(() => {
    let index = 0;
    const typingInterval = setInterval(() => {
      if (index <= fullText.length) {
        setTypingText(fullText.slice(0, index));
        index++;
      } else {
        setTimeout(() => {
          setTypingText('');
          index = 0;
        }, 2000);
      }
    }, 24);

    return () => clearInterval(typingInterval);
  }, []);

  return (
    <div className="w-full max-w-[1040px] mx-auto rounded-[2rem] border border-white/[0.12] bg-[linear-gradient(160deg,rgba(22,27,42,.95),rgba(10,12,19,.98))] shadow-[0_40px_120px_rgba(0,0,0,0.45)] overflow-hidden transform rotate-x-6 rotate-y-[-9deg] translate-y-3 perspective-[1700px]">
      <div className="flex items-center justify-between px-5 py-4 bg-gradient-to-b from-white/[0.05] to-white/[0.01] border-b border-white/[0.1]">
        <div className="flex items-center gap-4">
          <div className="flex gap-1.5">
            <span className="w-3 h-3 rounded-full bg-[#ef4444]" />
            <span className="w-3 h-3 rounded-full bg-[#eab308]" />
            <span className="w-3 h-3 rounded-full bg-[#22c55e]" />
          </div>
          <div className="text-[0.72rem] font-bold uppercase tracking-[0.16em] text-[var(--color-text-tertiary)]">DuckDocs Demo Console</div>
        </div>
        <span className="inline-flex items-center gap-2 text-[0.67rem] px-2.5 py-1 rounded-full border border-white/[0.16] bg-[rgba(93,168,255,0.12)] text-[var(--color-accent-blue)] font-semibold uppercase tracking-[0.1em]">
          <span className="w-1.5 h-1.5 rounded-full bg-[var(--color-accent-green)] animate-[pulse_2s_ease-in-out_infinite]" />
          Live preview
        </span>
      </div>

      <div className="p-5 grid gap-4 lg:grid-cols-[240px_minmax(0,1fr)_300px] min-w-0">
        <aside className="rounded-[1.25rem] bg-[var(--color-bg-elevated)] border border-white/[0.08] p-4 flex flex-col gap-4">
          <div className="text-[0.68rem] font-bold uppercase tracking-[0.18em] text-[var(--color-text-tertiary)]">Project workspace</div>
          <div className="rounded-[1rem] p-3.5 bg-[rgba(255,255,255,0.03)] border border-white/[0.07]">
            <div className="flex items-start gap-3">
              <div className="w-10 h-10 rounded-xl bg-gradient-to-b from-[#f5dcc3] to-[#d89ef8] flex items-center justify-center text-[0.66rem] font-black text-[#120f1d]">PDF</div>
              <div className="min-w-0">
                <div className="text-[0.97rem] font-bold text-[var(--color-text-primary)] truncate">{status.input.file}</div>
                <div className="text-xs text-[var(--color-text-tertiary)] mt-0.5">{status.input.pages} pages · {status.input.size}</div>
              </div>
            </div>
          </div>

          <div className="rounded-[1rem] bg-[rgba(12,16,27,0.75)] border border-white/[0.08] p-3">
            <div className="text-[0.7rem] font-bold uppercase tracking-[0.15em] text-[var(--color-text-tertiary)] mb-2">Profile</div>
            <div className="space-y-2">
              <div className="flex items-center justify-between text-[0.73rem]">
                <span className="text-[var(--color-text-secondary)]">Provider</span>
                <span className="text-[var(--color-text-primary)] font-medium">{status.provider}</span>
              </div>
              <div className="h-px bg-white/[0.07]" />
              <div className="flex items-center justify-between text-[0.73rem]">
                <span className="text-[var(--color-text-secondary)]">Template</span>
                <span className="text-[var(--color-text-primary)] font-medium">{status.template}</span>
              </div>
            </div>
          </div>

          <div className="space-y-2">
            {sectionTabs.map((tab) => (
              <button
                key={tab.key}
                type="button"
                onClick={() => setActiveSection(tab.key)}
                className={cn(
                  'w-full rounded-[0.75rem] px-3 py-2.5 text-left text-[0.74rem] font-semibold flex items-center justify-between border transition-colors',
                  activeSection === tab.key
                    ? 'bg-[rgba(168,85,247,0.16)] border-[rgba(177,108,255,0.35)] text-white'
                    : 'bg-[rgba(255,255,255,0.03)] border-white/[0.06] text-[var(--color-text-secondary)] hover:text-white hover:bg-white/[0.05]'
                )}
              >
                <span className="inline-flex items-center gap-2">
                  <tab.icon size={14} />
                  {tab.label}
                </span>
                <ArrowUpRight size={12} />
              </button>
            ))}
          </div>

          <div className="rounded-[1rem] bg-[rgba(8,10,16,.74)] border border-white/[0.08] p-3 mt-auto">
            <div className="text-[0.68rem] uppercase tracking-[0.14em] font-bold text-[var(--color-text-tertiary)] mb-2">Run notes</div>
            <div className="text-[0.76rem] text-[var(--color-text-secondary)] leading-6">
              Source pages are parsed in order. Results are assembled after confidence checks and then written as a markdown bundle.
            </div>
          </div>
        </aside>

        <main className="rounded-[1.25rem] bg-[var(--color-bg-elevated)] border border-white/[0.08] p-4 flex flex-col gap-4 min-w-0">
          <div className="rounded-[1rem] border border-white/[0.1] bg-[rgba(255,255,255,.02)] p-4">
            <div className="flex flex-wrap items-center justify-between gap-2 mb-3">
              <div className="text-[0.83rem] font-semibold text-[var(--color-text-primary)]">Pipeline health</div>
              <div className="text-[0.64rem] uppercase tracking-[0.16em] text-[var(--color-text-tertiary)] font-bold">{activeSection}</div>
            </div>

            <div className="grid sm:grid-cols-4 gap-3">
              {keyMetrics.map((metric) => (
                <div
                  key={metric.title}
                  className="rounded-[0.8rem] border border-white/[0.09] p-3"
                  style={{
                    background:
                      metric.tone === 'blue'
                        ? 'linear-gradient(170deg, rgba(104,166,255,0.12), rgba(104,166,255,0.02))'
                        : metric.tone === 'purple'
                          ? 'linear-gradient(170deg, rgba(177,108,255,0.16), rgba(177,108,255,0.02))'
                          : metric.tone === 'gold'
                            ? 'linear-gradient(170deg, rgba(240,216,174,0.2), rgba(240,216,174,0.02))'
                            : 'linear-gradient(170deg, rgba(86,215,234,0.2), rgba(86,215,234,0.02))',
                  }}
                >
                  <div className="text-[0.68rem] uppercase tracking-[0.13em] text-[var(--color-text-tertiary)] font-bold">{metric.title}</div>
                  <div className="mt-2 text-2xl font-black text-[var(--color-text-primary)]">{metric.value}</div>
                  <div className="text-[0.72rem] text-[var(--color-text-secondary)] mt-0.5">{metric.note}</div>
                </div>
              ))}
            </div>
          </div>

          <div className="flex-1 rounded-[1rem] bg-[rgba(12,15,24,.58)] border border-white/[0.08] p-4 min-h-0">
            <div className="grid sm:grid-cols-[minmax(0,1fr)_minmax(0,0.98fr)] gap-3">
              <div className="space-y-2">
                <div className="flex items-center justify-between mb-1">
                  <span className="text-[0.72rem] uppercase tracking-[0.14em] font-bold text-[var(--color-text-tertiary)]">Page extraction</span>
                  <span className="text-[0.7rem] text-[var(--color-text-tertiary)]">{progress}/18</span>
                </div>
                <div className="h-2.5 rounded-full bg-white/[0.06] overflow-hidden">
                  <div
                    className="h-full rounded-full bg-gradient-to-r from-[#c99fff] via-[#7aaeff] to-[#56d7ea] shadow-[0_0_24px_rgba(122,174,255,0.45)] transition-all duration-700"
                    style={{ width: `${(progress / 18) * 100}%` }}
                  />
                </div>

                <div className="space-y-2 pt-2">
                  {fileRows.map((row) => (
                    <div key={row.page} className="rounded-[0.75rem] border border-white/[0.08] bg-[rgba(255,255,255,0.03)] px-3 py-2.5 flex items-center gap-2">
                      <span className="w-11 font-mono text-xs text-[var(--color-text-tertiary)]">{row.page}</span>
                      <span className="text-sm text-[var(--color-text-primary)] flex-1 truncate">{row.section}</span>
                      <span
                        className={cn(
                          'text-[0.6rem] uppercase font-bold px-2 py-0.5 rounded-full border',
                          row.status === 'done' &&
                            'text-[var(--color-accent-green)] bg-[rgba(60,217,159,0.16)] border-[rgba(60,217,159,0.36)]',
                          row.status === 'live' &&
                            'text-[var(--color-accent-blue)] bg-[rgba(104,166,255,0.15)] border-[rgba(104,166,255,0.34)]',
                          row.status === 'queued' &&
                            'text-[var(--color-text-tertiary)] bg-[rgba(255,255,255,0.06)] border-[rgba(255,255,255,0.16)]'
                        )}
                      >
                        {row.status}
                      </span>
                    </div>
                  ))}
                </div>
              </div>

              <div className="rounded-[0.95rem] border border-white/[0.08] bg-[rgba(18,22,34,.72)] p-3">
                <div className="flex items-center justify-between text-[0.68rem] uppercase tracking-[0.1em] text-[var(--color-text-tertiary)] font-bold mb-2">
                  <span>OCR + AI stage timeline</span>
                  <TimerReset size={14} />
                </div>
                <div className="relative border border-white/[0.06] rounded-xl p-3 h-full min-h-[220px]">
                  <div className="text-[0.74rem] text-[var(--color-text-secondary)] mb-3">Recent checkpoints</div>
                  <div className="space-y-2 text-[0.78rem]">
                    <div className="rounded-lg bg-[rgba(60,217,159,0.1)] border border-[rgba(60,217,159,0.28)] px-2 py-2 flex items-center gap-2">
                      <CheckCircle2 size={14} />
                      page-01.png · heading blocks parsed
                    </div>
                    <div className="rounded-lg bg-[rgba(86,215,234,0.11)] border border-[rgba(86,215,234,0.28)] px-2 py-2 flex items-center gap-2">
                      <LoaderCircle size={14} className="animate-spin" />
                      pages-{String(livePage - 1).padStart(2, '0')} onward in progress
                    </div>
                    <div className="rounded-lg bg-[rgba(255,255,255,0.04)] border border-[rgba(255,255,255,0.12)] px-2 py-2 flex items-center gap-2">
                      <CircleDashed size={14} />
                      12 queued pages awaiting merge
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 mt-3">
              <ProcessItem label="Rasterize pages" state="done" />
              <ProcessItem label="AI extraction" state="live" />
              <ProcessItem label="Package output" state="queued" />
            </div>
          </div>
        </main>

        <aside className="rounded-[1.25rem] bg-[var(--color-bg-elevated)] border border-white/[0.08] p-4 flex flex-col gap-4">
          <div className="rounded-[1rem] border border-white/[0.1] bg-[rgba(255,255,255,0.03)] p-4">
            <div className="text-[0.7rem] uppercase tracking-[0.16em] text-[var(--color-text-tertiary)] font-bold mb-2">Generated package</div>
            <div className="rounded-lg border border-white/[0.1] bg-[rgba(9,11,18,.8)] p-2 mb-2">
              <div className="flex justify-between text-[0.65rem] text-[var(--color-text-secondary)] uppercase tracking-[0.08em]">
                <span>report.md</span>
                <span>18 pages</span>
              </div>
              <div className="mt-2 font-mono text-[0.77rem] space-y-1.5 text-[var(--color-text-secondary)]">
                <div className="md-line text-white/[0.28]">1</div>
                <div className="md-line text-[#f2dcb3]"># Q4 Operating Review</div>
                <div className="md-line text-[var(--color-text-secondary)]">Executive summary and extracted sections...</div>
                <div className="md-line text-[#86b8ff]">![Page 01](./images/page-01.png)</div>
                <div className="md-line text-[#f2dcb3]">## Highlights</div>
                <div className="md-line text-[var(--color-text-secondary)]">
                  {typingText}
                  <span className="inline-block w-0.5 h-4 bg-[rgba(134,184,255,0.9)] ml-1 animate-[blink_1s_step-end_infinite]" />
                </div>
              </div>
            </div>

            <a
              href="#download"
              className="mt-2 inline-flex w-full justify-center items-center gap-2 rounded-xl border border-[rgba(255,255,255,.16)] bg-[rgba(255,255,255,0.04)] text-[0.74rem] text-[var(--color-text-primary)] px-3 py-2.5 transition hover:bg-white/[0.08]"
            >
              <FileText size={14} />
              Open markdown preview
            </a>
          </div>

          <div className="rounded-[1rem] border border-white/[0.1] p-4 bg-[rgba(255,255,255,0.02)]">
            <div className="text-[0.7rem] uppercase tracking-[0.16em] text-[var(--color-text-tertiary)] font-bold mb-2">Artifacts</div>
            <div className="grid grid-cols-2 gap-2 text-[0.72rem]">
              <span className="rounded-lg border border-white/[0.1] px-2 py-2 text-[var(--color-text-secondary)]">page-01.png</span>
              <span className="rounded-lg border border-white/[0.1] px-2 py-2 text-[var(--color-text-secondary)]">page-02.png</span>
              <span className="rounded-lg border border-white/[0.1] px-2 py-2 text-[var(--color-text-secondary)]">page-03.png</span>
              <span className="rounded-lg border border-white/[0.1] px-2 py-2 text-[var(--color-text-secondary)]">summary.json</span>
            </div>
            <button
              type="button"
              className="mt-3 inline-flex w-full items-center justify-center gap-2 rounded-xl bg-[rgba(86,215,234,0.12)] border border-[rgba(86,215,234,0.34)] text-[0.72rem] px-2.5 py-2.5 text-[var(--color-text-primary)]"
            >
              <Download size={14} />
              Export package
            </button>
          </div>
        </aside>
      </div>
    </div>
  );
}
