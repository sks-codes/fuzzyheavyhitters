import json
import matplotlib.pyplot as plt
import numpy as np
from textwrap import fill

def load_experiments(file_path):
  """Load ndjson file into a list of experiment dicts"""
  experiments = []
  with open(file_path) as f:
    for line in f:
      try:
        experiments.append(json.loads(line))
      except json.JSONDecodeError as e:
        print(f"Skipping malformed line: {e}")
  return experiments

def plot_averaged_times(experiments, output_file="../data/time_vs_number_of_nodes.png"):
  """Plot averaged total_level_time vs nodes_searched"""
  plt.figure(figsize=(12, 8))

  for exp in experiments:
    exp_id = exp["metadata"]["experiment_id"]

    for i, (server1, server2) in enumerate(exp["server_side"]):
      nodes = server1["nodes_searched"]
      avg_time = (server1["total_level_time"] + server2["total_level_time"]) / 2

      plt.plot(
          nodes,
          avg_time,
          marker='o' if i == 0 else 's',  # Different marker for first level
          linestyle='-',
          label=f"Level {i+1}"
      )

  plt.xlabel("Nodes Searched")
  plt.ylabel("Average Server Search Time")
  plt.title("RideAustin Busiest Day - Averaged Server Performance")
  plt.grid(True)

  # Parameter annotation
  sample_params = experiments[0]["parameters"]
  param_text = "\n".join([
    f"Parameters:",
    f"Clients: {sample_params['num_clients']}",
    f"Dimensions: {sample_params['dimensions']}",
    f"Threshold: {sample_params['threshold']}",
    f"Ball Radius: {sample_params['ball_radius']}"
  ])

  plt.annotate(
      param_text,
      xy=(0.98, 0.5),
      xycoords='axes fraction',
      ha='right',
      va='center',
      bbox=dict(boxstyle='round', alpha=0.2))

  plt.legend(bbox_to_anchor=(1.05, 1), loc='upper left')
  plt.tight_layout()
  plt.savefig(output_file, dpi=300, bbox_inches='tight')
  print(f"Saved averaged plot to {output_file}")


def plot_stacked_time_components(experiments, output_file="../data/stacked_time_components.png"):
  """Stacked bar plot where each bar is a level, colored by time components"""
  plt.figure(figsize=(14, 8))

  # Prepare data structure
  components = ['FSS', 'GCequality', 'FieldActions', 'GCCompare']
  colors = ['#1f77b4', '#ff7f0e', '#2ca02c', '#d62728']
  level_names = []
  component_data = {comp: [] for comp in components}

  for exp in experiments:
    exp_id = exp["metadata"]["experiment_id"]
    for i, (server1, server2) in enumerate(exp["server_side"]):
      # Average the two servers' times
      level_names.append(f"{exp_id}\nL{i+1}")
      for comp in components:
        avg_time = (server1["time_breakdown"][comp] + server2["time_breakdown"][comp]) / 2
        component_data[comp].append(avg_time)

  # Create stacked bars
  x = np.arange(len(level_names))
  bottom = np.zeros(len(level_names))

  for comp, color in zip(components, colors):
    plt.bar(x, component_data[comp], width=0.8, bottom=bottom,
            label=comp, color=color, edgecolor='white')
    bottom += component_data[comp]

  # Formatting
  plt.xlabel('Level')
  plt.ylabel('Time (s)')
  plt.title('Time Breakdown by Level')
  plt.xticks(x, level_names, rotation=45, ha='right')

  # Add legend and parameter info
  plt.legend(title='Components', bbox_to_anchor=(1.05, 1), loc='upper left')

  sample_params = experiments[0]["parameters"]
  param_text = "\n".join([
    f"Parameters:",
    f"Clients: {sample_params['num_clients']}",
    f"Dimensions: {sample_params['dimensions']}",
    f"Threshold: {sample_params['threshold']}",
    f"Ball Radius: {sample_params['ball_radius']}"
  ])

  plt.annotate(
      param_text,
      xy=(0.98, 0.5),
      xycoords='axes fraction',
      ha='right',
      va='center',
      bbox=dict(boxstyle='round', alpha=0.2))

  plt.grid(True, axis='y')
  plt.tight_layout()
  plt.savefig(output_file, dpi=300, bbox_inches='tight')
  print(f"Saved stacked component plot to {output_file}")

def plot_stacked_time_components(experiments, output_file="../data/day_week_month_components.png"):
  """Stacked bar plot where each bar is a level, colored by time components"""
  plt.figure(figsize=(14, 8))

  # Prepare data structure
  components = ['FSS', 'GCequality', 'FieldActions', 'GCCompare']
  colors = ['#1f77b4', '#ff7f0e', '#2ca02c', '#d62728']
  level_names = []
  component_data = {comp: [] for comp in components}

  for exp in experiments:
    exp_id = exp["metadata"]["experiment_id"]
    for i, (server1, server2) in enumerate(exp["server_side"]):
      # Average the two servers' times
      level_names.append(f"{exp_id}\nL{i+1}")
      for comp in components:
        avg_time = (server1["time_breakdown"][comp] + server2["time_breakdown"][comp]) / 2
        component_data[comp].append(avg_time)

  # Create stacked bars
  x = np.arange(len(level_names))
  bottom = np.zeros(len(level_names))

  for comp, color in zip(components, colors):
    plt.bar(x, component_data[comp], width=0.8, bottom=bottom,
            label=comp, color=color, edgecolor='white')
    bottom += component_data[comp]

  # Formatting
  plt.xlabel('Level')
  plt.ylabel('Time (s)')
  plt.title('Time Breakdown by Level')
  plt.xticks(x, level_names, rotation=45, ha='right')

  # Add legend and parameter info
  plt.legend(title='Components', bbox_to_anchor=(1.05, 1), loc='upper left')

  sample_params = experiments[0]["parameters"]
  param_text = "\n".join([
    f"Parameters:",
    f"Clients: {sample_params['num_clients']}",
    f"Dimensions: {sample_params['dimensions']}",
    f"Threshold: {sample_params['threshold']}",
    f"Ball Radius: {sample_params['ball_radius']}"
  ])

  plt.annotate(
      param_text,
      xy=(0.98, 0.5),
      xycoords='axes fraction',
      ha='right',
      va='center',
      bbox=dict(boxstyle='round', alpha=0.2))

  plt.grid(True, axis='y')
  plt.tight_layout()
  plt.savefig(output_file, dpi=300, bbox_inches='tight')
  print(f"Saved stacked component plot to {output_file}")

def plot_component_percentages(experiments, output_file="../data/component_percentages.png"):
  """Pie chart showing overall time distribution by component"""
  plt.figure(figsize=(10, 10))

  # Updated component labels
  components = ['FSS', 'GCequality + OTconversion', 'FieldActions', 'GCCompare']
  colors = ['#1f77b4', '#ff7f0e', '#2ca02c', '#d62728']

  # Aggregate all times across all experiments and levels
  total_times = {comp: 0.0 for comp in components}

  for exp in experiments:
    for server1, server2 in exp["server_side"]:
      total_times['FSS'] += (server1["time_breakdown"]["FSS"] +
                             server2["time_breakdown"]["FSS"]) / 2
      total_times['GCequality + OTconversion'] += (server1["time_breakdown"]["GCequality"] +
                                                   server2["time_breakdown"]["GCequality"]) / 2
      total_times['FieldActions'] += (server1["time_breakdown"]["FieldActions"] +
                                      server2["time_breakdown"]["FieldActions"]) / 2
      total_times['GCCompare'] += (server1["time_breakdown"]["GCCompare"] +
                                   server2["time_breakdown"]["GCCompare"]) / 2

  # Convert to percentages
  total = sum(total_times.values())
  percentages = [100 * total_times[comp] / total for comp in components]

  # Create pie chart
  wedges, texts, autotexts = plt.pie(
      percentages,
      labels=components,
      colors=colors,
      autopct='%1.1f%%',
      startangle=90,
      pctdistance=0.85,
      wedgeprops={'edgecolor': 'white', 'linewidth': 1}
  )

  # Improve label appearance
  plt.setp(autotexts, size=12, weight='bold')
  plt.setp(texts, size=12)

  # Add title and experiment info
  plt.title('Overall Time Distribution by Component\n', pad=20)

  # Add parameter info
  sample_params = experiments[0]["parameters"]
  param_text = "\n".join([
    f"Parameters (first experiment):",
    f"Clients: {sample_params['num_clients']}",
    f"Dimensions: {sample_params['dimensions']}",
    f"Threshold: {sample_params['threshold']}",
    f"Ball Radius: {sample_params['ball_radius']}"
  ])

  plt.annotate(
      param_text,
      xy=(-0.3, -0.1),
      xycoords='axes fraction',
      ha='left',
      va='center',
      bbox=dict(boxstyle='round', alpha=0.2))

  plt.tight_layout()
  plt.savefig(output_file, dpi=300, bbox_inches='tight')
  print(f"Saved component percentage plot to {output_file}")


def plot_time_comparison(data, ball_radius=3, threshold_percent=2):
  day_experiments = []
  week_experiments = []
  month_experiments = []

  for entry in data:
    metadata = entry['metadata']
    params = entry['parameters']

    if (params['ball_radius'] == ball_radius and
        f"{threshold_percent}%" in metadata['experiment_id']):

      if "Day" in metadata['experiment_id']:
        day_experiments.append(entry)
      elif "Week" in metadata['experiment_id']:
        week_experiments.append(entry)
      elif "Month" in metadata['experiment_id']:
        month_experiments.append(entry)

  # Get the first experiment of each type (assuming there's only one per type)
  day_data = day_experiments[0] if day_experiments else None
  week_data = week_experiments[0] if week_experiments else None
  month_data = month_experiments[0] if month_experiments else None

  if not day_data and not week_data:
    print("No data found for the specified parameters")
    return

  # Prepare data for plotting
  time_categories = ['FSS', 'GCequality', 'FieldActions', 'GCCompare']
  datasets = []
  labels = []

  if day_data:
    # Sum up all server-side times for Day
    day_times = [0] * len(time_categories)
    for level in day_data['server_side'][0]:
      for run in level:
        breakdown = run['time_breakdown']
        for i, cat in enumerate(time_categories):
          day_times[i] += breakdown[cat]
    datasets.append(day_times)
    labels.append("Busiest Day")

  if week_data:
    # Sum up all server-side times for Week
    week_times = [0] * len(time_categories)
    for level in week_data['server_side'][0]:
      for run in level:
        breakdown = run['time_breakdown']
        for i, cat in enumerate(time_categories):
          week_times[i] += breakdown[cat]
    datasets.append(week_times)
    labels.append("Busiest Week")

  if month_data:
    # Sum up all server-side times for Week
    month_times = [0] * len(time_categories)
    for level in month_data['server_side'][0]:
      for run in level:
        breakdown = run['time_breakdown']
        for i, cat in enumerate(time_categories):
          month_times[i] += breakdown[cat]
    datasets.append(month_times)
    labels.append("Busiest Month")

  fig, ax = plt.subplots(figsize=(10, 6))
  bottom = np.zeros(len(datasets))

  for i, category in enumerate(time_categories):
    values = [data[i] for data in datasets]
    ax.bar(labels, values, bottom=bottom, label=category)
    bottom += values

  ax.set_title(f"Server-side Time Breakdown (Ball Radius {ball_radius}, {threshold_percent}% threshold)")
  ax.set_ylabel("Time (seconds)")
  ax.legend(title="Time Categories")

  plt.tight_layout()
  plt.show()

if __name__ == "__main__":
  experiments = load_experiments("../data/ride_austin_experiments.json")
  if experiments:
    plot_averaged_times(experiments)
    plot_stacked_time_components(experiments)
    plot_component_percentages(experiments)
    plot_time_comparison(experiments)
  else:
    print("No valid experiments found in the log file.")