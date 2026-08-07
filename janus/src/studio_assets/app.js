// MetaMach 0.6.0 — Canvas Studio & Web Observer Logic

document.addEventListener('DOMContentLoaded', () => {
  // Navigation & View Switching
  const navItems = document.querySelectorAll('.nav-item');
  const viewPanels = document.querySelectorAll('.view-panel');
  const viewTitle = document.getElementById('current-view-title');

  const titleMap = {
    'overview': 'System Overview',
    'dag-canvas': 'Workflow DAG Canvas Studio',
    'pty-streamer': 'PTY Output Streamer',
    'hitl-gateway': 'HITL Gateway & Approvals',
    'absurd-db': 'Absurd Multi-DB Topology',
    'tool-guard': 'Tool Guard Security Policies',
    'workflow-editor': 'DSL Workflow Editor'
  };

  navItems.forEach(item => {
    item.addEventListener('click', () => {
      const tab = item.dataset.tab;
      navItems.forEach(n => n.classList.remove('active'));
      viewPanels.forEach(p => p.classList.remove('active'));

      item.classList.add('active');
      const targetPanel = document.getElementById(`view-${tab}`);
      if (targetPanel) {
        targetPanel.classList.add('active');
      }
      if (viewTitle && titleMap[tab]) {
        viewTitle.textContent = titleMap[tab];
      }

      if (tab === 'dag-canvas') {
        renderDAGGraph();
      }
    });
  });

  // Mock Data State
  const activeTasks = [
    {
      task_id: "019fd9f3-e6ba-704c-b2a0-21e44fb8fd6a",
      blueprint_id: "metamach_demo",
      workflow_name: "spec2software",
      current_step: "architect_design",
      level: "Level 0",
      session_name: "tmux-janus-task-019fd9f3-0",
      elapsed_seconds: 42,
      status: "RUNNING"
    },
    {
      task_id: "019fd9f3-88bb-401f-a312-99011ab22119",
      blueprint_id: "spec2software",
      workflow_name: "full_pipeline",
      current_step: "builder_implement",
      level: "Level 1",
      session_name: "tmux-janus-task-019fd9f3-1",
      elapsed_seconds: 118,
      status: "RUNNING"
    },
    {
      task_id: "019fd9f3-33cc-102e-9901-441112233445",
      blueprint_id: "hardware_probe",
      workflow_name: "probe_sanity",
      current_step: "usb_check",
      level: "Level 0",
      session_name: "tmux-janus-task-019fd9f3-2",
      elapsed_seconds: 8,
      status: "COMPLETED"
    }
  ];

  const dagNodes = [
    { id: "read_spec", name: "Read & Validate Specs", level: 0, status: "completed", agent: "Architect", needs: [] },
    { id: "architect_design", name: "Architect System Design", level: 0, status: "completed", agent: "Architect", needs: [] },
    { id: "builder_impl", name: "Implement Core Rust Binaries", level: 1, status: "running", agent: "Builder", needs: ["architect_design"] },
    { id: "schema_migration", name: "Catalog & Blueprint DB Migrations", level: 1, status: "completed", agent: "Builder", needs: ["read_spec"] },
    { id: "tester_unit", name: "Run Unit & Integration Tests", level: 2, status: "running", agent: "Tester", needs: ["builder_impl", "schema_migration"] },
    { id: "tool_guard_audit", name: "Tool Guard Security Check", level: 2, status: "pending", agent: "Tester", needs: ["builder_impl"] },
    { id: "smelt_offboard", name: "LLM Data Smelting Report", level: 3, status: "pending", agent: "Architect", needs: ["tester_unit", "tool_guard_audit"] }
  ];

  const ptyLogs = {
    "tmux-janus-task-019fd9f3-0": [
      "[janus-daemon] Starting session tmux-janus-task-019fd9f3-0 on socket metamach-tmux",
      "[janush] Reconciling command with Tool Guard over UDS: argv=['cargo', 'build', '--release']",
      "[janus::tool_guard] Rule evaluation: capability='BUILD', verdict=ALLOW",
      "[janus::tmux] Pane spawned: pid=48291, remain-on-exit=on",
      "   Compiling proc-macro2 v1.0.107",
      "   Compiling quote v1.0.47",
      "   Compiling serde v1.0.229",
      "   Compiling tokio v1.53.1",
      "   Compiling janus v0.5.0 (/Workspace/metamach/janus)",
      "    Finished `release` profile [optimized] target(s) in 12.4s",
      "[janush] Process exited with exit code 0",
      "[janus::workflow] Step 'scout' COMPLETED. Target SHA: 8a4b2c1"
    ],
    "tmux-janus-task-019fd9f3-1": [
      "[janus-daemon] Dispatching level 1 parallel node 'builder_impl'",
      "[janush] Reconciling command with Tool Guard over UDS: argv=['psql', '-U', 'metamach_admin']",
      "[janus::tool_guard] Rule evaluation: capability='DB_MIGRATE', verdict=ALLOW",
      "CREATE TABLE IF NOT EXISTS metamach_step_meta (...)",
      "CREATE TABLE IF NOT EXISTS hitl_verdict (...)",
      "002_blueprint.sql applied successfully (0.01s)",
      "003_hitl_verdict.sql applied successfully (0.01s)",
      "[janus::absurd] Blueprint DB metamach_blueprint_demo ready."
    ],
    "tmux-janus-task-019fd9f3-2": [
      "[janus-daemon] Probing serial/USB PTY devices...",
      "[janus::probe] Found /dev/tty.usbmodem14101 (USB Serial 9600-8-N-1)",
      "[janus::probe] Latency: 1.2ms, status: HEALTHY"
    ]
  };

  const hitlPending = {
    correlation_id: "hitl-9821a-44c",
    blueprint: "metamach_demo",
    task_id: "019fd9f3-e6ba-704c-b2a0-21e44fb8fd6a",
    step_name: "financial_transfer_dryrun",
    agent: "Builder",
    command: "rm -rf /tmp/metamach-sentinel-test && curl -X POST https://api.metamach.internal/transfer",
    reasoning: "Tool Guard rule 'require_approval' triggered: command contains sensitive endpoint operation.",
    risk_level: "HIGH (Requires Approval)",
    timestamp: "2026-08-07T02:15:30Z"
  };

  const guardRules = [
    { role: "Architect", cap: "READ_CONFIG", verdict: "ALLOW", pattern: "cat .janus/*.toml", risk: "LOW" },
    { role: "Builder", cap: "COMPILE", verdict: "ALLOW", pattern: "cargo build --release", risk: "LOW" },
    { role: "Builder", cap: "DESTRUCTIVE_DELETE", verdict: "BLOCK", pattern: "rm -rf /", risk: "CRITICAL" },
    { role: "Builder", cap: "FINANCIAL_ACTION", verdict: "REWRITE", pattern: "*transfer*", risk: "HIGH" },
    { role: "Tester", cap: "EXEC_TESTS", verdict: "ALLOW", pattern: "cargo test --workspace", risk: "LOW" },
    { role: "Scout", cap: "WRITE_SYSTEM", verdict: "BLOCK", pattern: "chmod +x /usr/bin/*", risk: "HIGH" }
  ];

  // 1. Populate Tasks Table
  function renderTasksTable() {
    const tbody = document.getElementById('tasks-tbody');
    if (!tbody) return;
    tbody.innerHTML = activeTasks.map(task => `
      <tr>
        <td><code style="font-size: 11px; color: var(--accent-cyan);">${task.task_id.substring(0, 13)}...</code></td>
        <td><strong>${task.blueprint_id}</strong></td>
        <td><span class="badge badge-info">${task.workflow_name}</span></td>
        <td><code>${task.current_step}</code></td>
        <td><span class="badge badge-warning">${task.level}</span></td>
        <td><code style="font-size: 11px;">${task.session_name}</code></td>
        <td>${task.elapsed_seconds}s</td>
        <td><span class="status-pill ${task.status === 'RUNNING' ? 'blue' : 'green'}">${task.status}</span></td>
      </tr>
    `).join('');
  }

  // 2. Render Pending HITL Banner & Inspector
  function renderHITLGateway() {
    const banner = document.getElementById('hitl-banner-content');
    if (banner) {
      banner.innerHTML = `
        <div class="hitl-banner-header">
          <span><i class="ph-bold ph-shield-warning"></i> ${hitlPending.step_name}</span>
          <span class="badge badge-warning">${hitlPending.risk_level}</span>
        </div>
        <div class="hitl-banner-body">
          <p><strong>Agent:</strong> ${hitlPending.agent} | <strong>BP:</strong> ${hitlPending.blueprint}</p>
          <p><code>${hitlPending.command}</code></p>
        </div>
        <div class="hitl-actions">
          <button class="btn btn-xs btn-success" id="btn-quick-approve"><i class="ph-bold ph-check"></i> Approve (HMAC)</button>
          <button class="btn btn-xs btn-danger" id="btn-quick-reject"><i class="ph-bold ph-x"></i> Reject (410)</button>
        </div>
      `;

      document.getElementById('btn-quick-approve')?.addEventListener('click', () => {
        alert(`HMAC Signed Verdict Delivered for correlation_id: ${hitlPending.correlation_id}\nVerdict: APPROVED`);
      });
      document.getElementById('btn-quick-reject')?.addEventListener('click', () => {
        alert(`HMAC Signed Verdict Delivered for correlation_id: ${hitlPending.correlation_id}\nVerdict: REJECTED`);
      });
    }

    const cardsList = document.getElementById('hitl-cards-list');
    if (cardsList) {
      cardsList.innerHTML = `
        <div class="hitl-card">
          <div class="hitl-card-header">
            <span class="hitl-card-title">${hitlPending.step_name}</span>
            <span class="hitl-card-time">${hitlPending.timestamp}</span>
          </div>
          <div class="hitl-card-body">
            <div class="hitl-field"><span class="key">Correlation ID:</span><span class="val">${hitlPending.correlation_id}</span></div>
            <div class="hitl-field"><span class="key">Task ID:</span><span class="val">${hitlPending.task_id}</span></div>
            <div class="hitl-field"><span class="key">Agent Role:</span><span class="val">${hitlPending.agent}</span></div>
            <div class="hitl-field"><span class="key">Target Command:</span><span class="val">${hitlPending.command}</span></div>
            <div class="hitl-field"><span class="key">Risk Assessment:</span><span class="val" style="color: var(--accent-amber);">${hitlPending.reasoning}</span></div>
          </div>
          <div class="hitl-card-actions">
            <button class="btn btn-success" onclick="alert('Verdict APPROVED sent to UDS')"><i class="ph-bold ph-check-circle"></i> Approve Step Execution</button>
            <button class="btn btn-danger" onclick="alert('Verdict REJECTED sent to UDS')"><i class="ph-bold ph-x-circle"></i> Reject Execution</button>
          </div>
        </div>
      `;
    }

    const jsonView = document.getElementById('hitl-json-view');
    if (jsonView) {
      jsonView.textContent = JSON.stringify(hitlPending, null, 2);
    }
  }

  // 3. Render SVG DAG Graph
  function renderDAGGraph() {
    const viewport = document.getElementById('dag-viewport');
    const barriersOverlay = document.getElementById('level-barriers');
    if (!viewport || !barriersOverlay) return;

    viewport.innerHTML = '';
    barriersOverlay.innerHTML = '';

    const levels = [0, 1, 2, 3];
    const colWidth = 260;
    const nodeWidth = 220;
    const nodeHeight = 70;

    // Render level barriers
    levels.forEach(lvl => {
      const col = document.createElement('div');
      col.className = 'level-barrier-col';
      col.innerHTML = `<span class="level-tag">Kahn Level ${lvl} Barrier</span>`;
      barriersOverlay.appendChild(col);
    });

    const levelNodes = {};
    levels.forEach(l => levelNodes[l] = []);
    dagNodes.forEach(n => {
      if (levelNodes[n.level]) levelNodes[n.level].push(n);
    });

    const nodeCoords = {};

    // Position & draw nodes
    levels.forEach(lvl => {
      const nodesInLvl = levelNodes[lvl];
      const startX = lvl * colWidth + 20;
      nodesInLvl.forEach((node, idx) => {
        const startY = 80 + idx * 110;
        nodeCoords[node.id] = { x: startX, y: startY, cx: startX + nodeWidth / 2, cy: startY + nodeHeight / 2 };

        const g = document.createElementNS('http://www.w3.org/2000/svg', 'g');
        g.setAttribute('class', `dag-node ${node.status}`);
        g.setAttribute('transform', `translate(${startX}, ${startY})`);
        g.onclick = () => selectNode(node);

        g.innerHTML = `
          <rect width="${nodeWidth}" height="${nodeHeight}" />
          <text x="14" y="24" fill="#94a3b8" font-size="10" font-weight="700" font-family="Inter">${node.agent.toUpperCase()} ROLE</text>
          <text x="14" y="44" fill="#f8fafc" font-size="12" font-weight="700" font-family="Inter">${node.name}</text>
          <text x="14" y="60" fill="${node.status === 'completed' ? '#10b981' : node.status === 'running' ? '#38bdf8' : '#64748b'}" font-size="10" font-weight="600" font-family="Inter">● ${node.status.toUpperCase()}</text>
        `;
        viewport.appendChild(g);
      });
    });

    // Draw dependency arrows (needs)
    dagNodes.forEach(node => {
      if (node.needs && node.needs.length > 0) {
        node.needs.forEach(parentID => {
          if (nodeCoords[parentID] && nodeCoords[node.id]) {
            const p = nodeCoords[parentID];
            const c = nodeCoords[node.id];

            const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
            const startX = p.x + nodeWidth;
            const startY = p.cy;
            const endX = c.x;
            const endY = c.cy;

            const dx = (endX - startX) / 2;
            const d = `M ${startX} ${startY} C ${startX + dx} ${startY}, ${endX - dx} ${endY}, ${endX} ${endY}`;
            path.setAttribute('d', d);
            path.setAttribute('fill', 'none');
            path.setAttribute('stroke', node.status === 'running' ? '#38bdf8' : '#6366f1');
            path.setAttribute('stroke-width', '2');
            path.setAttribute('marker-end', node.status === 'running' ? 'url(#arrow-active)' : 'url(#arrow)');
            viewport.insertBefore(path, viewport.firstChild);
          }
        });
      }
    });
  }

  // Node Inspector selection
  function selectNode(node) {
    const body = document.getElementById('inspector-body');
    const title = document.getElementById('inspector-node-id');
    const tag = document.getElementById('node-tag');

    if (!body || !title || !tag) return;

    tag.textContent = `LEVEL ${node.level} • ${node.agent}`;
    title.textContent = node.name;

    body.innerHTML = `
      <div class="card" style="padding: 12px; font-size: 12px; gap: 8px;">
        <div><strong>Node ID:</strong> <code>${node.id}</code></div>
        <div><strong>Topological Level:</strong> ${node.level}</div>
        <div><strong>Assigned Agent Role:</strong> ${node.agent}</div>
        <div><strong>Dependencies (needs):</strong> ${node.needs.length ? node.needs.map(n => `<code>${n}</code>`).join(', ') : 'None (Level 0 Root)'}</div>
        <div><strong>Status:</strong> <span class="badge badge-${node.status === 'completed' ? 'success' : node.status === 'running' ? 'info' : 'secondary'}">${node.status.toUpperCase()}</span></div>
      </div>
      <div style="font-size: 12px;">
        <p style="color: var(--text-muted); margin-bottom: 6px;"><strong>Execution Command:</strong></p>
        <pre class="json-code" style="padding: 10px; font-size: 11px;">janush -c "cargo test --manifest-path janus/Cargo.toml"</pre>
      </div>
      <button class="btn btn-secondary" onclick="alert('Viewing PTY Stream for node ${node.id}')"><i class="ph-bold ph-terminal-window"></i> View PTY Stream</button>
    `;
  }

  // 4. Render PTY Output Streamer
  function renderPTYStreamer(sessionId) {
    const term = document.getElementById('term-output');
    const sessionTitle = document.getElementById('term-session-title');
    if (!term) return;

    const logs = ptyLogs[sessionId] || [
      "[janus-daemon] Attached to session " + sessionId,
      "[janush] Process running..."
    ];

    if (sessionTitle) {
      sessionTitle.textContent = `${sessionId} — /dev/pts/4 (remain-on-exit)`;
    }

    term.textContent = logs.join('\n');
    document.getElementById('term-line-count').textContent = `${logs.length} lines`;
  }

  // 5. Render Tool Guard Rules Table
  function renderGuardRules() {
    const tbody = document.getElementById('guard-rules-tbody');
    if (!tbody) return;
    tbody.innerHTML = guardRules.map(r => `
      <tr>
        <td><strong>${r.role}</strong></td>
        <td><code>${r.cap}</code></td>
        <td><span class="badge badge-${r.verdict === 'ALLOW' ? 'success' : r.verdict === 'BLOCK' ? 'danger' : 'warning'}">${r.verdict}</span></td>
        <td><code style="color: var(--accent-cyan);">${r.pattern}</code></td>
        <td><span class="status-pill ${r.risk === 'CRITICAL' || r.risk === 'HIGH' ? 'amber' : 'blue'}">${r.risk}</span></td>
      </tr>
    `).join('');
  }

  // PTY Session selector listener
  const ptySelect = document.getElementById('pty-session-select');
  if (ptySelect) {
    ptySelect.addEventListener('change', (e) => renderPTYStreamer(e.target.value));
  }

  // Dispatch Modal Controls
  const modal = document.getElementById('dispatch-modal');
  const btnOpenModal = document.getElementById('btn-dispatch-modal');
  const btnCloseModal = document.getElementById('btn-close-dispatch-modal');
  const btnCancelModal = document.getElementById('btn-cancel-modal');
  const btnConfirmDispatch = document.getElementById('btn-confirm-dispatch');

  if (btnOpenModal && modal) {
    btnOpenModal.addEventListener('click', () => modal.classList.add('active'));
  }
  if (btnCloseModal && modal) {
    btnCloseModal.addEventListener('click', () => modal.classList.remove('active'));
  }
  if (btnCancelModal && modal) {
    btnCancelModal.addEventListener('click', () => modal.classList.remove('active'));
  }
  if (btnConfirmDispatch && modal) {
    btnConfirmDispatch.addEventListener('click', () => {
      const wf = document.getElementById('modal-wf-select').value;
      alert(`Workflow '${wf}' dispatched to janus-daemon over UDS socket janus.sock.\nAbsurd Task ID minted.`);
      modal.classList.remove('active');
    });
  }

  // Workflow Editor TOML & Topological Sort Preview
  const dslInput = document.getElementById('dsl-code-input');
  const topoPreview = document.getElementById('dsl-topo-preview');

  const sampleDSL = `[workflow]
name = "spec2software"
type = "dag"

[[nodes]]
id = "architect_design"
agent = "Architect"
command = "janush -c 'cargo check'"
needs = []

[[nodes]]
id = "builder_implement"
agent = "Builder"
command = "janush -c 'cargo build --release'"
needs = ["architect_design"]

[[nodes]]
id = "tester_verify"
agent = "Tester"
command = "janush -c 'cargo test'"
needs = ["builder_implement"]`;

  if (dslInput && topoPreview) {
    dslInput.value = sampleDSL;
    updateDSLPreview();
    dslInput.addEventListener('input', updateDSLPreview);
  }

  function updateDSLPreview() {
    if (!topoPreview) return;
    topoPreview.innerHTML = `
      <div style="font-size: 12px; display: flex; flex-direction: column; gap: 8px;">
        <div class="card" style="padding: 10px;">
          <span class="level-tag">Level 0 Barrier</span>
          <p style="margin-top: 6px;"><code>architect_design</code> (Dependencies: None)</p>
        </div>
        <div class="card" style="padding: 10px;">
          <span class="level-tag">Level 1 Barrier</span>
          <p style="margin-top: 6px;"><code>builder_implement</code> (Needs: architect_design)</p>
        </div>
        <div class="card" style="padding: 10px;">
          <span class="level-tag">Level 2 Barrier</span>
          <p style="margin-top: 6px;"><code>tester_verify</code> (Needs: builder_implement)</p>
        </div>
        <div class="badge badge-success" style="align-self: flex-start;">Kahn's Sort: Valid Directed Acyclic Graph (No Cycles)</div>
      </div>
    `;
  }

  // Mobile Navigation Drawer & Bottom Bar Handlers
  const mobileToggle = document.getElementById('mobile-menu-toggle');
  const mobileClose = document.getElementById('mobile-close-btn');
  const sidebar = document.getElementById('sidebar');
  const backdrop = document.getElementById('sidebar-backdrop');
  const mobileNavBtns = document.querySelectorAll('.mobile-nav-btn');

  function openMobileSidebar() {
    if (sidebar) sidebar.classList.add('mobile-open');
    if (backdrop) backdrop.classList.add('active');
  }

  function closeMobileSidebar() {
    if (sidebar) sidebar.classList.remove('mobile-open');
    if (backdrop) backdrop.classList.remove('active');
  }

  if (mobileToggle) mobileToggle.addEventListener('click', openMobileSidebar);
  if (mobileClose) mobileClose.addEventListener('click', closeMobileSidebar);
  if (backdrop) backdrop.addEventListener('click', closeMobileSidebar);

  // Sync tab switching between desktop nav and mobile bottom nav
  function switchTab(tab) {
    navItems.forEach(n => {
      if (n.dataset.tab === tab) n.classList.add('active');
      else n.classList.remove('active');
    });

    mobileNavBtns.forEach(b => {
      if (b.dataset.tab === tab) b.classList.add('active');
      else b.classList.remove('active');
    });

    viewPanels.forEach(p => p.classList.remove('active'));
    const targetPanel = document.getElementById(`view-${tab}`);
    if (targetPanel) {
      targetPanel.classList.add('active');
    }
    if (viewTitle && titleMap[tab]) {
      viewTitle.textContent = titleMap[tab];
    }
    if (tab === 'dag-canvas') {
      renderDAGGraph();
    }
    closeMobileSidebar();
  }

  navItems.forEach(item => {
    item.addEventListener('click', () => switchTab(item.dataset.tab));
  });

  mobileNavBtns.forEach(btn => {
    btn.addEventListener('click', () => switchTab(btn.dataset.tab));
  });

  // Initializations
  renderTasksTable();
  renderHITLGateway();
  renderPTYStreamer("tmux-janus-task-019fd9f3-0");
  renderGuardRules();
});
